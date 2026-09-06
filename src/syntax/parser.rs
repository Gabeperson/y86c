use tinyvec::TinyVec;

use crate::common::span::Span;
use crate::common::symbol::Symbol;
use crate::common::{CallingConvention, Inline};
use crate::syntax::ast::*;
use crate::syntax::context::Context;
use crate::syntax::lexer::{KeywordKind, Token, TokenKind};

type Result<T> = std::result::Result<T, ParsingError>;

#[derive(Debug, Clone)]
pub enum ParsingError {
    ExpectedOtherToken {
        expected: TokenKind,
        found: Token,
        msg: &'static str,
    },
    ExpectedType {
        found: Token,
    },
    ExpectedArraySize {
        found: Token,
    },
    ExpectedExpr {
        found: Token,
    },
    ExpectedIdent {
        found: Token,
    },
    ExpectedReturnType {
        found: Token,
    },
    ExpectedGlobalDecl {
        found: Token,
    },
    IntLiteralOutOfRange {
        span: Span,
    },
    ExpectedAttrParam {
        found: Token,
    },
    UnexpectedEndOfInput,
    UnknownAttribute {
        for_decl: DeclKind,
        span: Span,
    },
    InvalidAttributeParam {
        for_decl: DeclKind,
        for_attr: Symbol,
        span: Span,
    },
    MissingAttributeParam {
        for_decl: DeclKind,
        for_attr: Symbol,
        span: Span,
    },
    ExternVarHasInitializer {
        span: Span,
        attr: Span,
        name: Ident,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum DeclKind {
    Function,
    Struct,
    Variable,
}

#[derive(Debug)]
pub struct Parser<'t> {
    tokens: &'t Vec<Token>,
    errors: Vec<ParsingError>,
    cursor: usize,
    id: u64,
    testing: bool,
}

#[derive(Debug, Clone)]
pub enum AttrParam {
    Ident(Ident),
    Num { int: u64, minus: bool, span: Span },
}

#[derive(Debug, Clone, Default)]
pub struct Attr {
    name: Ident,
    param: Option<AttrParam>,
}

#[derive(Debug)]
pub struct ParseOutput {
    pub program: Program,
    pub errors: Vec<ParsingError>,
}

impl ParseOutput {
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

impl<'t> Parser<'t> {
    pub fn parse_test(tokens: &'t Vec<Token>, ctx: &mut Context) -> ParseOutput {
        let mut parser = Parser::new(tokens);
        parser.testing = true;
        parser.parse_inner(ctx)
    }
    pub fn parse(tokens: &'t Vec<Token>, ctx: &mut Context) -> ParseOutput {
        let parser = Parser::new(tokens);
        parser.parse_inner(ctx)
    }
    fn new(tokens: &'t Vec<Token>) -> Self {
        Self {
            tokens,
            errors: Vec::new(),
            cursor: 0,
            id: 0,
            testing: false,
        }
    }
    fn parse_inner(mut self, ctx: &mut Context) -> ParseOutput {
        let mut decls = Vec::new();
        while !self.is_at_end() {
            let decl = self.parse_global_decl(ctx);
            match decl {
                Ok(decl) => {
                    decls.push(decl);
                }
                Err(e) => {
                    self.errors.push(e);
                    if let Err(e) = self.recover_global() {
                        self.errors.push(e);
                        break;
                    }
                }
            }
            if let Ok(true) = self.match_token(TokenKind::Semicolon) {
                self.advance();
            }
        }
        let program = Program { decls };
        ParseOutput {
            program,
            errors: self.errors,
        }
    }
    fn recover_global(&mut self) -> Result<()> {
        loop {
            let token = self.current()?;
            match token.kind {
                TokenKind::RCurly => {
                    let next = self.next()?;
                    if let TokenKind::Keyword(KeywordKind::Struct)
                    | TokenKind::Keyword(KeywordKind::Fn) = next.kind
                    {
                        self.advance();
                        return Ok(());
                    }
                }
                TokenKind::Semicolon => {
                    let next = self.next()?;
                    if let TokenKind::Keyword(KeywordKind::Struct)
                    | TokenKind::Keyword(KeywordKind::Fn) = next.kind
                    {
                        self.advance();
                        return Ok(());
                    }
                }
                _ => {}
            }
            self.advance()
        }
    }
    fn parse_struct_init(&mut self, ctx: &mut Context) -> Result<Expr> {
        let ident = self.parse_ident(ctx)?;
        self.expect(
            TokenKind::LCurly,
            "Expected '{' after struct name in struct initialization",
        )?;
        let mut field_inits = TinyVec::new();
        while !self.match_token(TokenKind::RCurly)? {
            let name = self.parse_ident(ctx)?;
            self.expect(
                TokenKind::Colon,
                "Expected ':' after member name in struct initialization",
            )?;
            let value = self.parse_expr(ctx)?;
            let value = ctx.intern_expr(value);
            field_inits.push((name, value));
            if self.match_token(TokenKind::Comma)? {
                self.advance();
            } else {
                break;
            }
        }
        let close_curly = self.expect(
            TokenKind::RCurly,
            "Expected '}' at end of struct initialization",
        )?;
        let span = Span::new(ident.span.start, close_curly.span.end);
        let id = self.next_id();
        let init = ExprKind::StructInit(StructInit {
            name: ident,
            field_inits,
            span,
            id,
        });
        Ok(Expr::new(init, span, id))
    }
    fn parse_array_init(&mut self, ctx: &mut Context) -> Result<Expr> {
        let token = self.expect_or_ice(TokenKind::LSquare);
        let mut elements = TinyVec::new();
        while !self.match_token(TokenKind::RSquare)? {
            let elem = self.parse_expr(ctx)?;
            let elem = ctx.intern_expr(elem);
            elements.push(elem);
            if self.match_token(TokenKind::Comma)? {
                self.advance();
            } else {
                break;
            }
        }
        let rsquare = self.expect(
            TokenKind::RSquare,
            "Expected ']' at end of array initialization",
        )?;
        let span = Span::new(token.span.start, rsquare.span.end);
        let id = self.next_id();
        let init = ExprKind::ArrayInit(ArrayInit { elements, span, id });
        Ok(Expr::new(init, span, id))
    }
    fn parse_attrs(&mut self, ctx: &mut Context) -> Result<TinyVec<[Attr; 5]>> {
        self.expect_or_ice(TokenKind::At);
        self.expect(
            TokenKind::LSquare,
            "Expected '[' after attribute start token '@'",
        )?;
        let mut v: TinyVec<[Attr; 5]> = TinyVec::new();
        while !self.match_token(TokenKind::RSquare)? {
            let attr_type = self.parse_ident(ctx)?;
            let attr_param = if self.match_token(TokenKind::LParen)? {
                self.advance();
                let param = self.parse_attr_param(ctx)?;
                self.expect(TokenKind::RParen, "Expected ')' after attribute parameter")?;
                Some(param)
            } else {
                None
            };
            v.push(Attr {
                name: attr_type,
                param: attr_param,
            });
            if self.match_token(TokenKind::Comma)? {
                self.advance()
            } else {
                break;
            }
        }
        self.expect(TokenKind::RSquare, "Expected ']' at end of attribute")?;
        Ok(v)
    }
    fn parse_global_decl(&mut self, ctx: &mut Context) -> Result<GlobalDeclaration> {
        let mut token = self.current()?;
        let attrs = if let TokenKind::At = &token.kind {
            let res = self.parse_attrs(ctx)?;
            token = self.current()?;
            Some(res)
        } else {
            None
        };
        let attrs = if let Some(attrs) = &attrs {
            attrs.as_slice()
        } else {
            &[]
        };
        match token.kind {
            TokenKind::Keyword(KeywordKind::Struct) => {
                let decl = self.parse_struct_decl(ctx, attrs)?;
                let span = decl.span;
                let id = decl.id;
                Ok(GlobalDeclaration {
                    kind: GlobalDeclarationKind::Struct(decl),
                    span,
                    id,
                })
            }
            TokenKind::Keyword(KeywordKind::Fn) => {
                let decl = self.parse_function_decl(ctx, attrs)?;
                let span = decl.span;
                let id = decl.id;
                Ok(GlobalDeclaration {
                    kind: GlobalDeclarationKind::Function(decl),
                    span,
                    id,
                })
            }
            TokenKind::Keyword(KeywordKind::Let) => {
                let decl = self.parse_variable_decl(ctx, attrs)?;
                if !self.match_token(TokenKind::Semicolon)? {
                    self.errors.push(ParsingError::ExpectedOtherToken {
                        expected: TokenKind::Semicolon,
                        found: self.current()?,
                        msg: "Expected ';' after global-level variable declaration",
                    });
                } else {
                    self.advance();
                }
                let span = decl.span;
                let id = decl.id;
                Ok(GlobalDeclaration {
                    kind: GlobalDeclarationKind::Variable(decl),
                    span,
                    id,
                })
            }
            _ => Err(ParsingError::ExpectedGlobalDecl { found: token }),
        }
    }
    fn parse_stmt(&mut self, ctx: &mut Context) -> Result<Stmt> {
        let token = self.current()?;
        let stmt = match token.kind {
            TokenKind::LCurly => {
                let block = self.parse_block(ctx)?;
                let span = block.span;
                let id = block.id;
                let kind = StmtKind::Block(block);
                return Ok(Stmt { kind, span, id });
            }
            TokenKind::Keyword(KeywordKind::Continue) => {
                self.advance();
                let span = token.span;
                let id = self.next_id();
                let kind = StmtKind::Continue(Continue { span, id });
                Stmt { kind, span, id }
            }
            TokenKind::Keyword(KeywordKind::Break) => {
                self.advance();
                let span = token.span;
                let id = self.next_id();
                let kind = StmtKind::Break(Break { span, id });
                Stmt { kind, span, id }
            }
            TokenKind::Keyword(KeywordKind::Return) => return self.parse_return_stmt(ctx),
            TokenKind::Keyword(KeywordKind::While) => return self.parse_while_loop(ctx),
            TokenKind::Keyword(KeywordKind::For) => return self.parse_for_loop(ctx),
            TokenKind::Keyword(KeywordKind::If) => return self.parse_if_stmt(ctx),
            TokenKind::Keyword(KeywordKind::Let) => {
                let decl = self.parse_variable_decl(ctx, &[])?;
                let span = decl.span;
                let id = decl.id;
                let kind = StmtKind::VariableDeclaration(decl);
                Stmt { kind, span, id }
            }
            TokenKind::Keyword(KeywordKind::Assert) if self.testing => self.parse_assert(ctx)?,
            _ => {
                let expr = self.parse_expr(ctx)?;
                let span = expr.span;
                let id = expr.id;
                let expr = ctx.intern_expr(expr);
                let kind = StmtKind::Expr(expr);
                Stmt { kind, span, id }
            }
        };
        if self.match_token(TokenKind::Semicolon)? {
            self.advance()
        } else {
            self.errors.push(ParsingError::ExpectedOtherToken {
                expected: TokenKind::Semicolon,
                found: self.current()?,
                msg: "Expected ';' after expression statement",
            });
            self.recover_stmt()?;
        }
        Ok(stmt)
    }
    fn parse_expr(&mut self, ctx: &mut Context) -> Result<Expr> {
        self.expr_bp(0, ctx)
    }
    fn recover_stmt(&mut self) -> Result<()> {
        loop {
            let token = self.current()?;
            match token.kind {
                TokenKind::Semicolon => {
                    self.advance();
                    match self.current()?.kind {
                        TokenKind::RParen | TokenKind::RSquare | TokenKind::Semicolon => {
                            self.advance();
                        }
                        _ => {
                            return Ok(());
                        }
                    }
                }
                TokenKind::RCurly => {
                    self.advance();
                    while let Ok(tok) = self.current()
                        && let TokenKind::Semicolon = tok.kind
                    {
                        self.advance();
                    }
                    return Ok(());
                }
                TokenKind::Keyword(KeywordKind::Let) => return Ok(()),
                _ => self.advance(),
            }
        }
    }
    fn parse_type_node(&mut self, ctx: &mut Context) -> Result<TypeNode> {
        let (typ, span) = self.parse_type(ctx)?;
        let id = self.next_id();

        Ok(TypeNode {
            inner: ctx.intern_type(typ),
            span,
            id,
        })
    }
    fn parse_type(&mut self, ctx: &mut Context) -> Result<(Type, Span)> {
        while self.current()?.kind == TokenKind::Error {
            self.advance();
        }
        let token = self.current()?;
        match token.kind {
            TokenKind::Keyword(KeywordKind::Int) => {
                self.advance();
                Ok((Type::Int, token.span))
            }
            TokenKind::Keyword(kind @ KeywordKind::Fn)
            | TokenKind::Keyword(kind @ KeywordKind::FnSys) => {
                self.advance();
                self.expect(
                    TokenKind::LParen,
                    "Expected '(' after 'fn' in function pointer",
                )?;
                let mut param_types = TinyVec::new();
                while !self.match_token(TokenKind::RParen)? {
                    let (typ, _span) = self.parse_type(ctx)?;
                    let typ = ctx.intern_type(typ);
                    param_types.push(typ);
                    if self.match_token(TokenKind::Comma)? {
                        self.advance()
                    } else {
                        break;
                    }
                }
                let end_tok = self.expect(
                    TokenKind::RParen,
                    "Expected ')' after parameter list in function pointer",
                )?;
                let mut span = Span::new(token.span.start, end_tok.span.end);
                let return_type = if self.match_token(TokenKind::Arrow)? {
                    self.advance();
                    let (typ, ret_span) = self.parse_type(ctx)?;
                    let typ = ctx.intern_type(typ);
                    span.end = ret_span.end;
                    typ
                } else {
                    ctx.intern_type(Type::Void)
                };
                Ok((
                    Type::FuncPtr {
                        return_type,
                        param_types,
                        kind: match kind {
                            KeywordKind::Fn => FnPtrKind::Internal,
                            KeywordKind::FnSys => FnPtrKind::Abi,
                            _ => unreachable!(),
                        },
                    },
                    span,
                ))
            }
            TokenKind::Keyword(KeywordKind::Void) => {
                self.advance();
                Ok((Type::Void, token.span))
            }
            TokenKind::Keyword(KeywordKind::NoAlias) => {
                self.advance();
                if !self.match_token(TokenKind::Asterisk)? {
                    return Err(ParsingError::ExpectedOtherToken {
                        expected: TokenKind::Asterisk,
                        found: token,
                        msg: "Expected pointer '*' after 'noalias'",
                    });
                }
                self.parse_pointer(token, true, ctx)
            }
            TokenKind::Asterisk => self.parse_pointer(token, false, ctx),
            TokenKind::Ident(_) => {
                let ident = self.parse_ident(ctx)?;
                Ok((Type::Struct { name: ident.sym }, ident.span))
            }
            TokenKind::LSquare => {
                self.advance();
                let (element_type, _element_span) = self.parse_type(ctx)?;
                let element_type = ctx.intern_type(element_type);
                self.expect(
                    TokenKind::Semicolon,
                    "Expected ';' in array type separating element type and size",
                )?;
                let token = self.current()?;
                let TokenKind::IntLiteral(len, _) = token.kind else {
                    return Err(ParsingError::ExpectedArraySize { found: token });
                };
                let len = if len > i64::MAX as u64 {
                    self.errors
                        .push(ParsingError::IntLiteralOutOfRange { span: token.span });
                    1
                } else {
                    len
                } as i64;

                self.advance();
                let end_tok =
                    self.expect(TokenKind::RSquare, "Expected ']' at end of array type")?;
                Ok((
                    Type::Array { element_type, len },
                    Span::new(token.span.start, end_tok.span.end),
                ))
            }
            TokenKind::Comma | TokenKind::Equal | TokenKind::Semicolon | TokenKind::RParen => {
                let span = token.span;
                self.errors
                    .push(ParsingError::ExpectedType { found: token });
                Ok((Type::Void, span))
            }
            _ => Err(ParsingError::ExpectedType { found: token }),
        }
    }

    fn parse_pointer(
        &mut self,
        token: Token,
        noalias: bool,
        ctx: &mut Context,
    ) -> Result<(Type, Span)> {
        let mut count = 0;
        while self.match_token(TokenKind::Asterisk)? {
            count += 1;
            self.advance();
        }
        let (mut typ, span) = self.parse_type(ctx)?;
        for i in 0..count {
            let is_last = i == count - 1;
            typ = Type::Ptr {
                pointee: ctx.intern_type(typ),
                noalias: if is_last { noalias } else { false },
            };
        }
        Ok((typ, Span::new(token.span.start, span.end)))
    }
    fn parse_assert(&mut self, ctx: &mut Context) -> Result<Stmt> {
        let token = self.current()?;
        self.advance();
        self.expect(TokenKind::LParen, "Expected '(' after assert")?;
        let expr = self.parse_expr(ctx)?;
        let rparen = self.expect(TokenKind::RParen, "Expected ')' after assert expression")?;
        let span = Span::new(token.span.start, rparen.span.end);
        let id = self.next_id();
        let kind = StmtKind::Assert(Assert {
            condition: ctx.intern_expr(expr),
            span,
            id,
        });
        Ok(Stmt { kind, span, id })
    }
    fn parse_ident_type_pair(&mut self, ctx: &mut Context) -> Result<(Ident, TypeNode)> {
        let ident = self.parse_ident(ctx)?;
        self.expect(
            TokenKind::Colon,
            "Expected ':' after ident in ident: type pair",
        )?;
        let typ = self.parse_type_node(ctx)?;
        Ok((ident, typ))
    }
    fn parse_variable_decl(
        &mut self,
        ctx: &mut Context,
        attrs: &[Attr],
    ) -> Result<VariableDeclaration> {
        let mut is_extern = false;
        let mut attr_span = Span::empty();
        for attr in attrs {
            let name = ctx.get_symbol(attr.name.sym);
            match name {
                "extern" => {
                    let None = &attr.param else {
                        self.errors.push(ParsingError::InvalidAttributeParam {
                            for_decl: DeclKind::Variable,
                            for_attr: attr.name.sym,
                            span: attr.name.span,
                        });
                        continue;
                    };
                    is_extern = true;
                    attr_span = attr.name.span;
                }
                _ => {
                    self.errors.push(ParsingError::UnknownAttribute {
                        for_decl: DeclKind::Function,
                        span: attr.name.span,
                    });
                }
            }
        }

        let let_kw = self.expect_or_ice(TokenKind::Keyword(KeywordKind::Let));
        let name = self.parse_ident(ctx)?;
        self.expect(
            TokenKind::Colon,
            "Expected ':' after variable name in variable declaration",
        )?;
        let typ = self.parse_type_node(ctx)?;
        let mut span = Span::new(let_kw.span.start, typ.span.end);
        let init_value = if let Ok(true) = self.match_token(TokenKind::Equal) {
            self.advance();
            let expr = self.parse_expr(ctx)?;
            span.end = expr.span.end;
            Some(expr)
        } else {
            None
        };
        if is_extern && let Some(init_value) = &init_value {
            self.errors.push(ParsingError::ExternVarHasInitializer {
                span: init_value.span,
                attr: attr_span,
                name,
            });
        }
        let id = self.next_id();
        Ok(VariableDeclaration {
            is_extern,
            var_type: typ,
            name,
            init_value: init_value.map(|v| ctx.intern_expr(v)),
            span,
            id,
        })
    }
    fn parse_function_decl(
        &mut self,
        ctx: &mut Context,
        attrs: &[Attr],
    ) -> Result<FunctionDeclaration> {
        let mut inline = Inline::Auto;
        let mut calling_convention = CallingConvention::Internal;

        for attr in attrs {
            let name = ctx.get_symbol(attr.name.sym);
            match name {
                "inline" => {
                    let Some(param) = &attr.param else {
                        self.errors.push(ParsingError::MissingAttributeParam {
                            for_decl: DeclKind::Function,
                            for_attr: attr.name.sym,
                            span: attr.name.span,
                        });
                        continue;
                    };
                    if let AttrParam::Ident(id) = param {
                        let param = ctx.get_symbol(id.sym);
                        match param {
                            "always" => {
                                inline = Inline::Always;
                                continue;
                            }
                            "never" => {
                                inline = Inline::Never;
                                continue;
                            }
                            _ => {}
                        }
                    }
                    self.errors.push(ParsingError::InvalidAttributeParam {
                        for_decl: DeclKind::Function,
                        for_attr: attr.name.sym,
                        span: attr.name.span,
                    });
                    continue;
                }
                "cc" => {
                    let Some(param) = &attr.param else {
                        self.errors.push(ParsingError::MissingAttributeParam {
                            for_decl: DeclKind::Function,
                            for_attr: attr.name.sym,
                            span: attr.name.span,
                        });
                        continue;
                    };
                    if let AttrParam::Ident(id) = param {
                        let param = ctx.get_symbol(id.sym);
                        match param {
                            "internal" => {
                                calling_convention = CallingConvention::Internal;
                                continue;
                            }
                            "abi" => {
                                calling_convention = CallingConvention::Abi;
                                continue;
                            }
                            _ => {}
                        }
                    }
                    self.errors.push(ParsingError::InvalidAttributeParam {
                        for_decl: DeclKind::Function,
                        for_attr: attr.name.sym,
                        span: attr.name.span,
                    });
                    continue;
                }
                _ => {
                    self.errors.push(ParsingError::UnknownAttribute {
                        for_decl: DeclKind::Function,
                        span: attr.name.span,
                    });
                }
            }
        }

        let fn_kw = self.expect_or_ice(TokenKind::Keyword(KeywordKind::Fn));
        let name = self.parse_ident(ctx)?;
        self.expect(
            TokenKind::LParen,
            "Expected '(' after function name in declaration",
        )?;
        let mut params = TinyVec::new();
        while !self.match_token(TokenKind::RParen)? {
            let pair = self.parse_ident_type_pair(ctx)?;
            params.push(pair);
            if self.match_token(TokenKind::Comma)? {
                self.advance();
            } else {
                break;
            }
        }
        self.expect(TokenKind::RParen, "Expected ')' after function parameters")?;
        let token = self.current()?;
        // This parses '-> void' as Some(Type::Void) and no return as in 'fn foo() {}' as
        // None. This is fine, as it's normalized in later passes to just Type::Void.
        let ret_type = match token.kind {
            TokenKind::LCurly => None,
            TokenKind::Arrow => {
                self.advance();
                Some(self.parse_type_node(ctx)?)
            }
            _ => {
                return Err(ParsingError::ExpectedReturnType { found: token });
            }
        };
        let body = self.parse_block(ctx)?;
        let span = Span::new(fn_kw.span.start, body.span.end);
        let id = self.next_id();
        Ok(FunctionDeclaration {
            inline,
            calling_convention,
            return_type: ret_type,
            name,
            params,
            body,
            span,
            id,
        })
    }
    fn parse_struct_decl(
        &mut self,
        ctx: &mut Context,
        attrs: &[Attr],
    ) -> Result<StructDeclaration> {
        for attr in attrs {
            self.errors.push(ParsingError::UnknownAttribute {
                for_decl: DeclKind::Struct,
                span: attr.name.span,
            });
        }
        let struct_kw = self.expect_or_ice(TokenKind::Keyword(KeywordKind::Struct));
        let name = self.parse_ident(ctx)?;
        self.expect(TokenKind::LCurly, "Expected '{' after struct name")?;
        let mut members = TinyVec::new();
        while !self.match_token(TokenKind::RCurly)? {
            let pair = self.parse_ident_type_pair(ctx)?;
            members.push(pair);
            if self.match_token(TokenKind::Comma)? {
                self.advance()
            } else {
                break;
            }
        }
        let rcurly = self.expect(
            TokenKind::RCurly,
            "Expected '}' at end of struct declaration",
        )?;
        let span = Span::new(struct_kw.span.start, rcurly.span.end);
        let id = self.next_id();
        Ok(StructDeclaration {
            name,
            fields: members,
            span,
            id,
        })
    }
    fn parse_block(&mut self, ctx: &mut Context) -> Result<Block> {
        let lcurly = self.expect(TokenKind::LCurly, "Expected '{' to start block")?;
        let mut statements = TinyVec::new();
        while !self.match_token(TokenKind::RCurly)? {
            match self.parse_stmt(ctx) {
                Ok(s) => statements.push(ctx.intern_stmt(s)),
                Err(e) => {
                    self.recover_stmt()?;
                    self.errors.push(e);
                }
            }
        }
        let end = self.expect(TokenKind::RCurly, "Expected '}' at end of block")?;
        let span = Span::new(lcurly.span.start, end.span.end);
        let id = self.next_id();
        Ok(Block {
            body: statements,
            span,
            id,
        })
    }
    fn parse_if_stmt(&mut self, ctx: &mut Context) -> Result<Stmt> {
        let if_kw = self.expect_or_ice(TokenKind::Keyword(KeywordKind::If));
        self.expect(TokenKind::LParen, "Expected '(' after 'if'")?;
        let condition = self.parse_expr(ctx)?;
        self.expect(TokenKind::RParen, "Expected ')' after if condition")?;
        let then_branch = self.parse_stmt(ctx)?;
        let mut span = Span::new(if_kw.span.start, then_branch.span.end);
        let else_branch = if self.match_token(TokenKind::Keyword(KeywordKind::Else))? {
            self.advance();
            let stmt = self.parse_stmt(ctx)?;
            span.end = stmt.span.end;
            Some(stmt)
        } else {
            None
        };
        let id = self.next_id();
        let kind = StmtKind::IfStmt(IfStmt {
            condition: ctx.intern_expr(condition),
            then_branch: ctx.intern_stmt(then_branch),
            else_branch: else_branch.map(|s| ctx.intern_stmt(s)),
            span,
            id,
        });
        Ok(Stmt { kind, span, id })
    }
    fn parse_while_loop(&mut self, ctx: &mut Context) -> Result<Stmt> {
        let while_kw = self.expect_or_ice(TokenKind::Keyword(KeywordKind::While));
        self.expect(TokenKind::LParen, "Expected '(' after 'while'")?;
        let condition = self.parse_expr(ctx)?;
        self.expect(TokenKind::RParen, "Expected ')' after while loop condition")?;
        let body = self.parse_stmt(ctx)?;
        let span = Span::new(while_kw.span.start, body.span.end);
        let id = self.next_id();
        let kind = StmtKind::WhileLoop(WhileLoop {
            condition: ctx.intern_expr(condition),
            body: ctx.intern_stmt(body),
            span,
            id,
        });
        Ok(Stmt { kind, span, id })
    }
    fn parse_for_loop(&mut self, ctx: &mut Context) -> Result<Stmt> {
        let for_kw = self.expect_or_ice(TokenKind::Keyword(KeywordKind::For));
        self.expect(TokenKind::LParen, "Expected '(' after 'for'")?;
        let token = self.current()?;
        let init = match token.kind {
            TokenKind::Keyword(KeywordKind::Let) => {
                let vardecl = self.parse_variable_decl(ctx, &[])?;
                let span = vardecl.span;
                let id = vardecl.id;
                let kind = StmtKind::VariableDeclaration(vardecl);
                Some(Stmt { kind, span, id })
            }
            TokenKind::Semicolon => None,
            _ => {
                let expr = self.parse_expr(ctx)?;
                let span = expr.span;
                let id = expr.id;
                let expr = ctx.intern_expr(expr);
                let kind = StmtKind::Expr(expr);
                Some(Stmt { kind, span, id })
            }
        };
        self.expect(TokenKind::Semicolon, "Expected ';' after loop initializer")?;
        let condition = match self.match_token(TokenKind::Semicolon)? {
            false => Some(self.parse_expr(ctx)?),
            true => None,
        };
        self.expect(TokenKind::Semicolon, "Expected ';' after loop condition")?;
        let post = match self.match_token(TokenKind::RParen)? {
            false => Some(self.parse_expr(ctx)?),
            true => None,
        };
        self.expect(TokenKind::RParen, "Expected ')' after for loop increment")?;
        let body = self.parse_stmt(ctx)?;
        let span = Span::new(for_kw.span.start, body.span.end);
        let id = self.next_id();
        let kind = StmtKind::ForLoop(ForLoop {
            init: init.map(|s| ctx.intern_stmt(s)),
            condition: condition.map(|e| ctx.intern_expr(e)),
            post: post.map(|e| ctx.intern_expr(e)),
            body: ctx.intern_stmt(body),
            span,
            id,
        });
        Ok(Stmt { kind, span, id })
    }
    fn parse_return_stmt(&mut self, ctx: &mut Context) -> Result<Stmt> {
        let return_kw = self.expect_or_ice(TokenKind::Keyword(KeywordKind::Return));
        if self.match_token(TokenKind::Semicolon)? {
            self.advance();
            let id = self.next_id();
            let stmt = StmtKind::ReturnStmt(ReturnStmt {
                value: None,
                span: return_kw.span,
                id,
            });
            return Ok(Stmt {
                kind: stmt,
                span: return_kw.span,
                id,
            });
        }
        let value = self.parse_expr(ctx)?;
        self.expect(TokenKind::Semicolon, "Expected ';' after return stmt")?;
        let span = Span::new(return_kw.span.start, value.span.end);

        let id = self.next_id();
        let stmt = StmtKind::ReturnStmt(ReturnStmt {
            value: Some(ctx.intern_expr(value)),
            span,
            id,
        });
        Ok(Stmt {
            kind: stmt,
            span,
            id,
        })
    }
    fn parse_ident(&mut self, _ctx: &mut Context) -> Result<Ident> {
        let token = self.current()?;
        let TokenKind::Ident(symbol) = token.kind else {
            return Err(ParsingError::ExpectedIdent { found: token });
        };
        self.advance();
        let id = self.next_id();
        Ok(Ident {
            sym: symbol,
            span: token.span,
            id,
        })
    }
    fn parse_attr_param(&mut self, ctx: &mut Context) -> Result<AttrParam> {
        let current = self.current()?;
        match current.kind {
            TokenKind::Ident(_) => Ok(AttrParam::Ident(self.parse_ident(ctx)?)),
            TokenKind::IntLiteral(num, _) => {
                self.advance();
                Ok(AttrParam::Num {
                    int: num,
                    minus: false,
                    span: current.span,
                })
            }
            TokenKind::Minus => {
                let next = self.next()?;
                if let TokenKind::IntLiteral(num, _) = next.kind {
                    self.advancen(2);
                    Ok(AttrParam::Num {
                        int: num,
                        minus: true,
                        span: Span::new(current.span.start, next.span.end),
                    })
                } else {
                    Err(ParsingError::ExpectedAttrParam { found: current })
                }
            }
            _ => Err(ParsingError::ExpectedAttrParam { found: current }),
        }
    }
}

impl<'t> Parser<'t> {
    fn expr_bp(&mut self, min_bp: u8, ctx: &mut Context) -> Result<Expr> {
        let token = self.current()?;
        let mut lhs = if let Some(((), bp)) = prefix_binding_power(&token.kind) {
            self.advance();
            let expr = self.expr_bp(bp, ctx)?;
            self.handle_prefix(token, expr, ctx)?
        } else {
            let Some(atom) = self.parse_atom(ctx)? else {
                return Err(ParsingError::ExpectedExpr { found: token });
            };
            atom
        };
        loop {
            if self.is_at_end() {
                break;
            }
            let token = match self.current() {
                Ok(t) => t,
                Err(_e) => break,
            };
            if let Some((bp, ())) = postfix_binding_power(&token.kind) {
                if bp < min_bp {
                    break;
                }
                lhs = self.handle_postfix(lhs, token, ctx)?;
                continue;
            }
            if let Some((l_bp, r_bp)) = infix_binding_power(&token.kind) {
                if l_bp < min_bp {
                    break;
                }
                self.advance();
                if let TokenKind::QuestionMark = token.kind {
                    let true_branch = self.parse_expr(ctx)?;
                    self.expect(TokenKind::Colon, "Expected ':' in ternary operator")?;
                    let false_branch = self.expr_bp(r_bp, ctx)?;
                    let condition = lhs;
                    let span = Span::new(condition.span.start, false_branch.span.end);
                    let id = self.next_id();
                    let kind = ExprKind::Ternary(Ternary {
                        condition: ctx.intern_expr(condition),
                        true_branch: ctx.intern_expr(true_branch),
                        false_branch: ctx.intern_expr(false_branch),
                        span,
                        id,
                    });
                    lhs = Expr::new(kind, span, id);
                    continue;
                }
                let rhs = self.expr_bp(r_bp, ctx)?;
                lhs = self.handle_infix(lhs, token, rhs, ctx)?;
                continue;
            }
            break;
        }
        Ok(lhs)
    }
    fn parse_atom(&mut self, ctx: &mut Context) -> Result<Option<Expr>> {
        let token = self.current()?;
        match token.kind {
            TokenKind::Keyword(KeywordKind::Sizeof) => {
                self.advance();
                self.expect(TokenKind::LParen, "Expected '(' after sizeof")?;
                let typ = self.parse_type_node(ctx)?;
                let end_tok = self.expect(TokenKind::RParen, "Expected ')' after sizeof type")?;
                let span = Span::new(token.span.start, end_tok.span.end);
                let id = self.next_id();
                let kind = ExprKind::SizeOfType(SizeOfType { typ, span, id });
                Ok(Some(Expr::new(kind, span, id)))
            }
            TokenKind::Keyword(KeywordKind::NewProvenance) => {
                self.advance();
                self.expect(TokenKind::LParen, "Expected '(' after new_prov")?;
                let ptr = self.parse_expr(ctx)?;
                let end_tok = self.expect(TokenKind::RParen, "Expected ')' at end of new_prov")?;
                let span = Span::new(token.span.start, end_tok.span.end);
                let id = self.next_id();
                let ptr = ctx.intern_expr(ptr);
                let kind = ExprKind::NewProvenance(NewProvenance { ptr, span, id });
                Ok(Some(Expr::new(kind, span, id)))
            }
            TokenKind::Keyword(KeywordKind::UnexposeProv) => {
                self.advance();
                self.expect(TokenKind::LParen, "Expected '(' after unexpose_prov")?;
                let ptr = self.parse_expr(ctx)?;
                let end_tok =
                    self.expect(TokenKind::RParen, "Expected ')' at end of unexpose_prov")?;
                let span = Span::new(token.span.start, end_tok.span.end);
                let id = self.next_id();
                let int = ctx.intern_expr(ptr);
                let kind = ExprKind::UnexposeProvenance(UnexposeProvenance { int, span, id });
                Ok(Some(Expr::new(kind, span, id)))
            }
            TokenKind::Keyword(KeywordKind::ExposeProvenance) => {
                self.advance();
                self.expect(TokenKind::LParen, "Expected '(' after expose_prov")?;
                let ptr = self.parse_expr(ctx)?;
                let end_tok =
                    self.expect(TokenKind::RParen, "Expected ')' at end of expose_prov")?;
                let span = Span::new(token.span.start, end_tok.span.end);
                let id = self.next_id();
                let ptr = ctx.intern_expr(ptr);
                let kind = ExprKind::ExposeProvenance(ExposeProvenance { ptr, span, id });
                Ok(Some(Expr::new(kind, span, id)))
            }
            TokenKind::Keyword(KeywordKind::CopyProvenance) => {
                self.advance();
                self.expect(TokenKind::LParen, "Expected '(' after copy_prov")?;
                let prov_ptr = self.parse_expr(ctx)?;
                self.expect(TokenKind::Comma, "Expected ',' after ptr in copy_prov")?;
                let addr = self.parse_expr(ctx)?;
                let end_tok = self.expect(TokenKind::RParen, "Expected ')' at end of copy_prov")?;
                let span = Span::new(token.span.start, end_tok.span.end);
                let id = self.next_id();
                let prov_ptr = ctx.intern_expr(prov_ptr);
                let addr = ctx.intern_expr(addr);
                let kind = ExprKind::CopyProvenance(CopyProvenance {
                    prov_ptr,
                    addr,
                    span,
                    id,
                });
                Ok(Some(Expr::new(kind, span, id)))
            }
            TokenKind::Keyword(KeywordKind::Nullptr) => {
                self.advance();
                let id = self.next_id();
                let kind = ExprKind::Nullptr(Nullptr {
                    span: token.span,
                    id,
                });
                Ok(Some(Expr::new(kind, token.span, id)))
            }
            TokenKind::LParen => {
                self.advance();
                let expr = self.parse_expr(ctx)?;
                self.expect(TokenKind::RParen, "Expected ')' after parenthesized expr")?;
                Ok(Some(expr))
            }
            TokenKind::LSquare => Ok(Some(self.parse_array_init(ctx)?)),
            TokenKind::IntLiteral(int, radix) => {
                self.advance();
                let span = token.span;
                let value: i64 = match (int, radix) {
                    v @ (0..=9_223_372_036_854_775_808, 10) => v.0 as i64,
                    v @ (_, 16 | 2) => v.0 as i64,
                    _ => {
                        self.errors
                            .push(ParsingError::IntLiteralOutOfRange { span: token.span });
                        0
                    }
                };
                let id = self.next_id();
                let kind = ExprKind::Int(Int {
                    lit: value,
                    span,
                    id,
                    radix,
                });
                Ok(Some(Expr::new(kind, span, id)))
            }
            TokenKind::Ident(s) => {
                if let Ok(Token {
                    kind: TokenKind::LCurly,
                    ..
                }) = self.next()
                {
                    Ok(Some(self.parse_struct_init(ctx)?))
                } else {
                    self.advance();
                    let id = self.next_id();
                    let kind = ExprKind::Ident(Ident {
                        sym: s,
                        span: token.span,
                        id,
                    });
                    Ok(Some(Expr::new(kind, token.span, id)))
                }
            }
            _ => Ok(None),
        }
    }

    fn handle_prefix(&mut self, op: Token, expr: Expr, ctx: &mut Context) -> Result<Expr> {
        let kind = match op.kind {
            TokenKind::DoublePlus => PrefixOpKind::Increment,
            TokenKind::DoubleMinus => PrefixOpKind::Decrement,
            TokenKind::Plus => PrefixOpKind::UnaryPlus,
            TokenKind::Minus => PrefixOpKind::UnaryMinus,
            TokenKind::Ampersand => PrefixOpKind::AddressOf,
            TokenKind::Asterisk => PrefixOpKind::Dereference,
            TokenKind::ExclamationMark => PrefixOpKind::Not,
            TokenKind::Tilde => PrefixOpKind::BitNot,
            _ => unreachable!(),
        };
        let span = Span::new(op.span.start, expr.span.end);
        let id = self.next_id();
        let kind = ExprKind::PrefixOp(PrefixOp {
            kind,
            expr: ctx.intern_expr(expr),
            span,
            id,
        });
        Ok(Expr::new(kind, span, id))
    }
    fn handle_infix(&mut self, lhs: Expr, op: Token, rhs: Expr, ctx: &mut Context) -> Result<Expr> {
        let binop_kind = match op.kind {
            TokenKind::Asterisk => BinaryOpKind::Mul,
            TokenKind::Slash => BinaryOpKind::Div,
            TokenKind::Percent => BinaryOpKind::Mod,
            TokenKind::Plus => BinaryOpKind::Add,
            TokenKind::Minus => BinaryOpKind::Sub,
            TokenKind::GreaterThan => BinaryOpKind::Greater,
            TokenKind::LessThan => BinaryOpKind::Less,
            TokenKind::GreaterOrEqual => BinaryOpKind::GreaterOrEqual,
            TokenKind::LessOrEqual => BinaryOpKind::LessOrEqual,
            TokenKind::DoubleEqual => BinaryOpKind::Eq,
            TokenKind::ExclamationMarkEqual => BinaryOpKind::NotEq,
            TokenKind::Ampersand => BinaryOpKind::BitAnd,
            TokenKind::Caret => BinaryOpKind::Xor,
            TokenKind::Pipe => BinaryOpKind::BitOr,
            TokenKind::DoubleAmpersand => BinaryOpKind::And,
            TokenKind::DoublePipe => BinaryOpKind::Or,
            TokenKind::PlusEqual => BinaryOpKind::AddAssign,
            TokenKind::MinusEqual => BinaryOpKind::SubAssign,
            TokenKind::AsteriskEqual => BinaryOpKind::MulAssign,
            TokenKind::SlashEqual => BinaryOpKind::DivAssign,
            TokenKind::AmpersandEqual => BinaryOpKind::BitAndAssign,
            TokenKind::PipeEqual => BinaryOpKind::BitOrAssign,
            TokenKind::CaretEqual => BinaryOpKind::XorAssign,
            TokenKind::PercentEqual => BinaryOpKind::ModAssign,
            TokenKind::Equal => BinaryOpKind::Assign,
            TokenKind::Shl => BinaryOpKind::Shl,
            TokenKind::Shr => BinaryOpKind::Shr,
            TokenKind::ShlEquals => BinaryOpKind::ShlAssign,
            TokenKind::ShrEquals => BinaryOpKind::ShrAssign,
            _ => unreachable!(),
        };
        let span = Span::new(lhs.span.start, rhs.span.end);
        let id = self.next_id();
        let exprkind = ExprKind::BinaryOp(BinaryOp {
            kind: binop_kind,
            left: ctx.intern_expr(lhs),
            right: ctx.intern_expr(rhs),
            span,
            id,
        });
        Ok(Expr::new(exprkind, span, id))
    }
    fn handle_postfix(&mut self, expr: Expr, op: Token, ctx: &mut Context) -> Result<Expr> {
        self.advance();
        match op.kind {
            TokenKind::LParen => {
                let mut args = TinyVec::new();
                while !self.match_token(TokenKind::RParen)? {
                    let expr = self.parse_expr(ctx)?;
                    args.push(ctx.intern_expr(expr));
                    if self.match_token(TokenKind::Comma)? {
                        self.advance();
                    } else {
                        break;
                    }
                }
                let rparen = self.expect(TokenKind::RParen, "Expected ')' after argument list")?;
                let span = Span::new(expr.span.start, rparen.span.end);
                let id = self.next_id();
                let kind = ExprKind::FunctionCall(FunctionCall {
                    func_expr: ctx.intern_expr(expr),
                    args,
                    span,
                    id,
                });
                Ok(Expr::new(kind, span, id))
            }
            TokenKind::LSquare => {
                let index = self.parse_expr(ctx)?;
                let rsquare = self.expect(
                    TokenKind::RSquare,
                    "Expected ']' after array index expression",
                )?;
                let span = Span::new(expr.span.start, rsquare.span.end);
                let id = self.next_id();
                let kind = ExprKind::ArrayIndex(ArrayIndex {
                    array: ctx.intern_expr(expr),
                    index: ctx.intern_expr(index),
                    span,
                    id,
                });
                Ok(Expr::new(kind, span, id))
            }
            TokenKind::Period => {
                let ident = self.parse_ident(ctx)?;
                let span = Span::new(expr.span.start, ident.span.end);
                let id = self.next_id();
                let kind = ExprKind::MemberAccess(MemberAccess {
                    struct_expr: ctx.intern_expr(expr),
                    member_name: ident,
                    span,
                    id,
                });
                Ok(Expr::new(kind, span, id))
            }
            TokenKind::Arrow => {
                let ident = self.parse_ident(ctx)?;
                let span = Span::new(expr.span.start, ident.span.end);
                let id = self.next_id();
                let kind = ExprKind::PointerMemberAccess(PointerMemberAccess {
                    struct_ptr_expr: ctx.intern_expr(expr),
                    member_name: ident,
                    span,
                    id,
                });
                Ok(Expr::new(kind, span, id))
            }
            TokenKind::DoublePlus | TokenKind::DoubleMinus => {
                let span = Span::new(expr.span.start, op.span.end);
                let id = self.next_id();
                let kind = ExprKind::PostfixOp(PostfixOp {
                    kind: if op.kind == TokenKind::DoublePlus {
                        PostfixOpKind::Increment
                    } else {
                        PostfixOpKind::Decrement
                    },
                    expr: ctx.intern_expr(expr),
                    span,
                    id,
                });
                Ok(Expr::new(kind, span, id))
            }
            TokenKind::Keyword(KeywordKind::As) => {
                let typ = self.parse_type_node(ctx)?;
                let span = Span::new(expr.span.start, typ.span.end);
                let id = self.next_id();
                let kind = ExprKind::Cast(Cast {
                    to_type: typ,
                    expr: ctx.intern_expr(expr),
                    span,
                    id,
                });
                Ok(Expr::new(kind, span, id))
            }
            _ => unreachable!(),
        }
    }
}

fn prefix_binding_power(token: &TokenKind) -> Option<((), u8)> {
    match token {
        TokenKind::Plus
        | TokenKind::Minus
        | TokenKind::Asterisk
        | TokenKind::Ampersand
        | TokenKind::ExclamationMark
        | TokenKind::DoublePlus
        | TokenKind::DoubleMinus
        | TokenKind::Tilde => Some(((), 130)),
        _ => None,
    }
}
fn infix_binding_power(token: &TokenKind) -> Option<(u8, u8)> {
    Some(match token {
        TokenKind::Asterisk | TokenKind::Slash | TokenKind::Percent => (110, 111),
        TokenKind::Plus | TokenKind::Minus => (100, 101),
        TokenKind::Shl | TokenKind::Shr => (94, 95),
        TokenKind::GreaterThan
        | TokenKind::LessThan
        | TokenKind::GreaterOrEqual
        | TokenKind::LessOrEqual => (90, 91),
        TokenKind::DoubleEqual | TokenKind::ExclamationMarkEqual => (80, 81),
        TokenKind::Ampersand => (70, 71),
        TokenKind::Caret => (60, 61),
        TokenKind::Pipe => (50, 51),
        TokenKind::DoubleAmpersand => (40, 41),
        TokenKind::DoublePipe => (30, 31),
        TokenKind::QuestionMark => (20, 19),
        TokenKind::PlusEqual
        | TokenKind::MinusEqual
        | TokenKind::AsteriskEqual
        | TokenKind::SlashEqual
        | TokenKind::AmpersandEqual
        | TokenKind::PipeEqual
        | TokenKind::CaretEqual
        | TokenKind::PercentEqual
        | TokenKind::Equal
        | TokenKind::ShlEquals
        | TokenKind::ShrEquals => (10, 9),
        _ => return None,
    })
}
fn postfix_binding_power(token: &TokenKind) -> Option<(u8, ())> {
    Some(match token {
        TokenKind::LParen | TokenKind::LSquare | TokenKind::Period | TokenKind::Arrow => (200, ()),
        TokenKind::DoublePlus | TokenKind::DoubleMinus => (140, ()),
        TokenKind::Keyword(KeywordKind::As) => (120, ()),
        _ => return None,
    })
}

impl<'t> Parser<'t> {
    fn next_id(&mut self) -> NodeId {
        let id = NodeId(self.id);
        self.id += 1;
        id
    }
    fn is_at_end(&self) -> bool {
        self.cursor >= self.tokens.len()
    }
    fn current(&self) -> Result<Token> {
        if self.cursor < self.tokens.len() {
            Ok(self.tokens[self.cursor].clone())
        } else {
            Err(ParsingError::UnexpectedEndOfInput)
        }
    }
    fn next(&self) -> Result<Token> {
        if self.cursor + 1 < self.tokens.len() {
            Ok(self.tokens[self.cursor + 1].clone())
        } else {
            Err(ParsingError::UnexpectedEndOfInput)
        }
    }
    fn advance(&mut self) {
        self.advancen(1)
    }
    fn advancen(&mut self, n: usize) {
        self.cursor += n
    }
    fn match_token(&self, token: TokenKind) -> Result<bool> {
        Ok(!self.is_at_end() && self.current()?.kind == token)
    }
    fn expect(&mut self, token: TokenKind, msg: &'static str) -> Result<Token> {
        let current = self.current()?;
        if current.kind != token {
            return Err(ParsingError::ExpectedOtherToken {
                expected: token,
                found: current,
                msg,
            });
        }
        self.advance();
        Ok(current)
    }
    #[track_caller]
    fn expect_or_ice(&mut self, token: TokenKind) -> Token {
        let current = self.current().expect("Internal Compiler Error");
        if current.kind != token {
            unreachable!("Internal Compiler Error")
        }
        self.advance();
        current
    }
}

#[cfg(test)]
mod tests {

    use crate::syntax::lexer::Lexer;

    use super::*;
    #[macro_use]
    mod utils {
        use std::cell::{RefCell, RefMut};

        use super::*;
        pub fn lex(s: &str, ctx: &mut Context) -> Vec<Token> {
            let lexed = Lexer::lex(s, ctx);
            assert!(!lexed.has_errors());
            lexed.tokens
        }

        thread_local! {
            static CTX: &'static RefCell<Context> = Box::leak(Box::new(RefCell::new(Context::new())));
        }

        #[track_caller]
        pub fn ctx() -> RefMut<'static, Context> {
            CTX.with(|ctx| ctx.borrow_mut())
        }

        pub fn num(n: i64) -> Expr {
            Expr::new(
                ExprKind::Int(Int {
                    radix: 10,
                    lit: n,
                    span: Span::empty(),
                    id: NodeId(0),
                }),
                Span::empty(),
                NodeId(0),
            )
        }
        pub fn a() -> Expr {
            Expr::new(ExprKind::Ident(ident("a")), Span::empty(), NodeId(0))
        }
        pub fn b() -> Expr {
            Expr::new(ExprKind::Ident(ident("b")), Span::empty(), NodeId(0))
        }
        pub fn c() -> Expr {
            Expr::new(ExprKind::Ident(ident("c")), Span::empty(), NodeId(0))
        }
        pub fn d() -> Expr {
            Expr::new(ExprKind::Ident(ident("d")), Span::empty(), NodeId(0))
        }
        pub fn e() -> Expr {
            Expr::new(ExprKind::Ident(ident("e")), Span::empty(), NodeId(0))
        }
        pub fn f() -> Expr {
            Expr::new(ExprKind::Ident(ident("f")), Span::empty(), NodeId(0))
        }
        pub fn g() -> Expr {
            Expr::new(ExprKind::Ident(ident("g")), Span::empty(), NodeId(0))
        }
        pub fn i() -> Expr {
            Expr::new(ExprKind::Ident(ident("i")), Span::empty(), NodeId(0))
        }
        pub fn ident(s: &str) -> Ident {
            Ident {
                sym: ctx().intern_symbol(s),
                span: Span::empty(),
                id: NodeId(0),
            }
        }
        pub fn ptr(pointee: Type) -> Type {
            Type::Ptr {
                pointee: ctx().intern_type(pointee),
                noalias: false,
            }
        }
        pub fn noalias_ptr(pointee: Type) -> Type {
            Type::Ptr {
                pointee: ctx().intern_type(pointee),
                noalias: true,
            }
        }
        pub fn void() -> Type {
            Type::Void
        }
        pub fn int() -> Type {
            Type::Int
        }
        pub fn struct_(s: Ident) -> Type {
            Type::Struct { name: s.sym }
        }
        pub fn array(t: Type, len: i64) -> Type {
            Type::Array {
                element_type: ctx().intern_type(t),
                len,
            }
        }
        pub fn typenode(t: Type, ctx: &mut Context) -> TypeNode {
            TypeNode {
                inner: ctx.intern_type(t),
                span: Span::empty(),
                id: NodeId(1),
            }
        }
        pub fn func_ptr(param_types: Vec<Type>, return_type: Type, kind: FnPtrKind) -> Type {
            let mut v = TinyVec::new();
            let ctx = &mut ctx();
            for i in param_types {
                v.push(ctx.intern_type(i))
            }
            let return_type = ctx.intern_type(return_type);
            Type::FuncPtr {
                return_type,
                kind,
                param_types: v,
            }
        }
        pub fn cast(from: Expr, to: Type) -> Expr {
            let mut ctx = ctx();
            Expr::new(
                ExprKind::Cast(Cast {
                    to_type: typenode(to, &mut ctx),
                    expr: ctx.intern_expr(from),
                    span: Span::empty(),
                    id: NodeId(0),
                }),
                Span::empty(),
                NodeId(0),
            )
        }
        pub fn binop(lhs: Expr, rhs: Expr, op_type: BinaryOpKind) -> Expr {
            let mut ctx = ctx();
            Expr::new(
                ExprKind::BinaryOp(BinaryOp {
                    kind: op_type,
                    left: ctx.intern_expr(lhs),
                    right: ctx.intern_expr(rhs),
                    span: Span::empty(),
                    id: NodeId(0),
                }),
                Span::empty(),
                NodeId(0),
            )
        }
        pub fn prefix_op(expr: Expr, op_type: PrefixOpKind) -> Expr {
            Expr::new(
                ExprKind::PrefixOp(PrefixOp {
                    kind: op_type,
                    expr: ctx().intern_expr(expr),
                    span: Span::empty(),
                    id: NodeId(0),
                }),
                Span::empty(),
                NodeId(0),
            )
        }
        pub fn postfix_op(expr: Expr, op_type: PostfixOpKind) -> Expr {
            Expr::new(
                ExprKind::PostfixOp(PostfixOp {
                    kind: op_type,
                    expr: ctx().intern_expr(expr),
                    span: Span::empty(),
                    id: NodeId(0),
                }),
                Span::empty(),
                NodeId(0),
            )
        }
        pub fn ternary(condition: Expr, true_branch: Expr, false_branch: Expr) -> Expr {
            let mut ctx = ctx();
            Expr::new(
                ExprKind::Ternary(Ternary {
                    condition: ctx.intern_expr(condition),
                    true_branch: ctx.intern_expr(true_branch),
                    false_branch: ctx.intern_expr(false_branch),
                    span: Span::empty(),
                    id: NodeId(0),
                }),
                Span::empty(),
                NodeId(0),
            )
        }
        pub fn func_call(func_expr: Expr, args: Vec<Expr>) -> Expr {
            let mut ctx = ctx();
            Expr::new(
                ExprKind::FunctionCall(FunctionCall {
                    func_expr: ctx.intern_expr(func_expr),
                    args: args.into_iter().map(|e| ctx.intern_expr(e)).collect(),
                    span: Span::empty(),
                    id: NodeId(0),
                }),
                Span::empty(),
                NodeId(0),
            )
        }
        pub fn member_access(expr: Expr, member: Ident) -> Expr {
            Expr::new(
                ExprKind::MemberAccess(MemberAccess {
                    struct_expr: ctx().intern_expr(expr),
                    member_name: member,
                    span: Span::empty(),
                    id: NodeId(0),
                }),
                Span::empty(),
                NodeId(0),
            )
        }
        pub fn pointer_member_access(expr: Expr, member: Ident) -> Expr {
            Expr::new(
                ExprKind::PointerMemberAccess(PointerMemberAccess {
                    struct_ptr_expr: ctx().intern_expr(expr),
                    member_name: member,
                    span: Span::empty(),
                    id: NodeId(0),
                }),
                Span::empty(),
                NodeId(0),
            )
        }
        pub fn array_index(expr: Expr, index: Expr) -> Expr {
            let mut ctx = ctx();
            Expr::new(
                ExprKind::ArrayIndex(ArrayIndex {
                    array: ctx.intern_expr(expr),
                    index: ctx.intern_expr(index),
                    span: Span::empty(),
                    id: NodeId(0),
                }),
                Span::empty(),
                NodeId(0),
            )
        }
        pub fn sizeof(typ: Type) -> Expr {
            let mut ctx = ctx();
            Expr::new(
                ExprKind::SizeOfType(SizeOfType {
                    typ: typenode(typ, &mut ctx),
                    span: Span::empty(),
                    id: NodeId(0),
                }),
                Span::empty(),
                NodeId(0),
            )
        }
        pub fn expose_prov(ptr: Expr) -> Expr {
            let mut ctx = ctx();
            Expr::new(
                ExprKind::ExposeProvenance(ExposeProvenance {
                    ptr: ctx.intern_expr(ptr),
                    span: Span::empty(),
                    id: NodeId(0),
                }),
                Span::empty(),
                NodeId(0),
            )
        }
        pub fn new_prov(ptr: Expr) -> Expr {
            let mut ctx = ctx();
            Expr::new(
                ExprKind::NewProvenance(NewProvenance {
                    ptr: ctx.intern_expr(ptr),
                    span: Span::empty(),
                    id: NodeId(0),
                }),
                Span::empty(),
                NodeId(0),
            )
        }
        pub fn unexpose_prov(int: Expr) -> Expr {
            let mut ctx = ctx();
            Expr::new(
                ExprKind::UnexposeProvenance(UnexposeProvenance {
                    int: ctx.intern_expr(int),
                    span: Span::empty(),
                    id: NodeId(0),
                }),
                Span::empty(),
                NodeId(0),
            )
        }
        pub fn copy_prov(prov_ptr: Expr, addr: Expr) -> Expr {
            let mut ctx = ctx();
            Expr::new(
                ExprKind::CopyProvenance(CopyProvenance {
                    prov_ptr: ctx.intern_expr(prov_ptr),
                    addr: ctx.intern_expr(addr),
                    span: Span::empty(),
                    id: NodeId(0),
                }),
                Span::empty(),
                NodeId(0),
            )
        }
        pub fn nullptr() -> Expr {
            Expr::new(
                ExprKind::Nullptr(Nullptr {
                    span: Span::empty(),
                    id: NodeId(0),
                }),
                Span::empty(),
                NodeId(0),
            )
        }
        pub fn get_type(s: &str) -> Type {
            let mut ctx = ctx();
            let tokens = lex(s, &mut ctx);
            let mut parser = Parser::new(&tokens);
            let output = parser.parse_type(&mut ctx).unwrap();
            assert!(parser.is_at_end(), "{parser:?}");
            output.0
        }
        pub fn add(lhs: Expr, rhs: Expr) -> Expr {
            binop(lhs, rhs, BinaryOpKind::Add)
        }
        pub fn sub(lhs: Expr, rhs: Expr) -> Expr {
            binop(lhs, rhs, BinaryOpKind::Sub)
        }
        pub fn mul(lhs: Expr, rhs: Expr) -> Expr {
            binop(lhs, rhs, BinaryOpKind::Mul)
        }
        pub fn div(lhs: Expr, rhs: Expr) -> Expr {
            binop(lhs, rhs, BinaryOpKind::Div)
        }
        pub fn shl(lhs: Expr, rhs: Expr) -> Expr {
            binop(lhs, rhs, BinaryOpKind::Shl)
        }
        pub fn shr(lhs: Expr, rhs: Expr) -> Expr {
            binop(lhs, rhs, BinaryOpKind::Shr)
        }
        pub fn eq(lhs: Expr, rhs: Expr) -> Expr {
            binop(lhs, rhs, BinaryOpKind::Eq)
        }
        pub fn gt(lhs: Expr, rhs: Expr) -> Expr {
            binop(lhs, rhs, BinaryOpKind::Greater)
        }
        pub fn lt(lhs: Expr, rhs: Expr) -> Expr {
            binop(lhs, rhs, BinaryOpKind::Less)
        }
        pub fn geq(lhs: Expr, rhs: Expr) -> Expr {
            binop(lhs, rhs, BinaryOpKind::GreaterOrEqual)
        }
        pub fn leq(lhs: Expr, rhs: Expr) -> Expr {
            binop(lhs, rhs, BinaryOpKind::LessOrEqual)
        }
        pub fn noteq(lhs: Expr, rhs: Expr) -> Expr {
            binop(lhs, rhs, BinaryOpKind::NotEq)
        }
        pub fn and(lhs: Expr, rhs: Expr) -> Expr {
            binop(lhs, rhs, BinaryOpKind::And)
        }
        pub fn or(lhs: Expr, rhs: Expr) -> Expr {
            binop(lhs, rhs, BinaryOpKind::Or)
        }
        pub fn bitand(lhs: Expr, rhs: Expr) -> Expr {
            binop(lhs, rhs, BinaryOpKind::BitAnd)
        }
        pub fn bitor(lhs: Expr, rhs: Expr) -> Expr {
            binop(lhs, rhs, BinaryOpKind::BitOr)
        }
        pub fn xor(lhs: Expr, rhs: Expr) -> Expr {
            binop(lhs, rhs, BinaryOpKind::Xor)
        }
        pub fn mod_(lhs: Expr, rhs: Expr) -> Expr {
            binop(lhs, rhs, BinaryOpKind::Mod)
        }
        pub fn addeq(lhs: Expr, rhs: Expr) -> Expr {
            binop(lhs, rhs, BinaryOpKind::AddAssign)
        }
        pub fn subeq(lhs: Expr, rhs: Expr) -> Expr {
            binop(lhs, rhs, BinaryOpKind::SubAssign)
        }
        pub fn muleq(lhs: Expr, rhs: Expr) -> Expr {
            binop(lhs, rhs, BinaryOpKind::MulAssign)
        }
        pub fn diveq(lhs: Expr, rhs: Expr) -> Expr {
            binop(lhs, rhs, BinaryOpKind::DivAssign)
        }
        pub fn shleq(lhs: Expr, rhs: Expr) -> Expr {
            binop(lhs, rhs, BinaryOpKind::ShlAssign)
        }
        pub fn shreq(lhs: Expr, rhs: Expr) -> Expr {
            binop(lhs, rhs, BinaryOpKind::ShrAssign)
        }
        pub fn andeq(lhs: Expr, rhs: Expr) -> Expr {
            binop(lhs, rhs, BinaryOpKind::BitAndAssign)
        }
        pub fn oreq(lhs: Expr, rhs: Expr) -> Expr {
            binop(lhs, rhs, BinaryOpKind::BitOrAssign)
        }
        pub fn xoreq(lhs: Expr, rhs: Expr) -> Expr {
            binop(lhs, rhs, BinaryOpKind::XorAssign)
        }
        pub fn modeq(lhs: Expr, rhs: Expr) -> Expr {
            binop(lhs, rhs, BinaryOpKind::ModAssign)
        }
        pub fn assign(lhs: Expr, rhs: Expr) -> Expr {
            binop(lhs, rhs, BinaryOpKind::Assign)
        }
        pub fn prefix_increment(expr: Expr) -> Expr {
            prefix_op(expr, PrefixOpKind::Increment)
        }
        pub fn prefix_decrement(expr: Expr) -> Expr {
            prefix_op(expr, PrefixOpKind::Decrement)
        }
        pub fn unary_plus(expr: Expr) -> Expr {
            prefix_op(expr, PrefixOpKind::UnaryPlus)
        }
        pub fn unary_minus(expr: Expr) -> Expr {
            prefix_op(expr, PrefixOpKind::UnaryMinus)
        }
        pub fn addr_of(expr: Expr) -> Expr {
            prefix_op(expr, PrefixOpKind::AddressOf)
        }
        pub fn dereference(expr: Expr) -> Expr {
            prefix_op(expr, PrefixOpKind::Dereference)
        }
        pub fn not(expr: Expr) -> Expr {
            prefix_op(expr, PrefixOpKind::Not)
        }
        pub fn bitnot(expr: Expr) -> Expr {
            prefix_op(expr, PrefixOpKind::BitNot)
        }

        pub fn suffix_increment(expr: Expr) -> Expr {
            postfix_op(expr, PostfixOpKind::Increment)
        }
        pub fn suffix_decrement(expr: Expr) -> Expr {
            postfix_op(expr, PostfixOpKind::Decrement)
        }
        pub fn struct_init(name: Ident, inits: Vec<(Ident, Expr)>) -> Expr {
            let mut ctx = ctx();
            Expr::new(
                ExprKind::StructInit(StructInit {
                    name,
                    field_inits: inits
                        .into_iter()
                        .map(|(i, e)| (i, ctx.intern_expr(e)))
                        .collect(),
                    span: Span::empty(),
                    id: NodeId(0),
                }),
                Span::empty(),
                NodeId(0),
            )
        }
        pub fn array_init(exprs: Vec<Expr>) -> Expr {
            let mut ctx = ctx();
            Expr::new(
                ExprKind::ArrayInit(ArrayInit {
                    elements: exprs.into_iter().map(|e| ctx.intern_expr(e)).collect(),
                    span: Span::empty(),
                    id: NodeId(0),
                }),
                Span::empty(),
                NodeId(0),
            )
        }
        pub fn return_stmt(r: Option<Expr>) -> Stmt {
            let mut ctx = ctx();
            Stmt {
                kind: StmtKind::ReturnStmt(ReturnStmt {
                    value: r.map(|e| ctx.intern_expr(e)),
                    span: Span::empty(),
                    id: NodeId(0),
                }),
                span: Span::empty(),
                id: NodeId(0),
            }
        }
        pub fn block(stmts: Vec<Stmt>) -> Stmt {
            let mut ctx = ctx();
            Stmt {
                kind: StmtKind::Block(Block {
                    body: stmts.into_iter().map(|s| ctx.intern_stmt(s)).collect(),
                    span: Span::empty(),
                    id: NodeId(0),
                }),
                span: Span::empty(),
                id: NodeId(0),
            }
        }
        pub fn bblock(stmts: Vec<Stmt>) -> Block {
            let mut ctx = ctx();
            Block {
                body: stmts.into_iter().map(|s| ctx.intern_stmt(s)).collect(),
                span: Span::empty(),
                id: NodeId(0),
            }
        }
        pub fn break_() -> Stmt {
            Stmt {
                kind: StmtKind::Break(Break {
                    span: Span::empty(),
                    id: NodeId(0),
                }),
                span: Span::empty(),
                id: NodeId(0),
            }
        }
        pub fn continue_() -> Stmt {
            Stmt {
                kind: StmtKind::Continue(Continue {
                    span: Span::empty(),
                    id: NodeId(0),
                }),
                span: Span::empty(),
                id: NodeId(0),
            }
        }
        pub fn if_stmt(condition: Expr, then_branch: Stmt, else_branch: Option<Stmt>) -> Stmt {
            let mut ctx = ctx();
            Stmt {
                kind: StmtKind::IfStmt(IfStmt {
                    condition: ctx.intern_expr(condition),
                    then_branch: ctx.intern_stmt(then_branch),
                    else_branch: else_branch.map(|s| ctx.intern_stmt(s)),
                    span: Span::empty(),
                    id: NodeId(0),
                }),
                span: Span::empty(),
                id: NodeId(0),
            }
        }
        pub fn while_loop(condition: Expr, body: Stmt) -> Stmt {
            let mut ctx = ctx();
            Stmt {
                kind: StmtKind::WhileLoop(WhileLoop {
                    condition: ctx.intern_expr(condition),
                    body: ctx.intern_stmt(body),
                    span: Span::empty(),
                    id: NodeId(0),
                }),
                span: Span::empty(),
                id: NodeId(0),
            }
        }
        pub fn for_loop(
            init: Option<Stmt>,
            condition: Option<Expr>,
            post: Option<Expr>,
            body: Stmt,
        ) -> Stmt {
            let mut ctx = ctx();
            Stmt {
                kind: StmtKind::ForLoop(ForLoop {
                    init: init.map(|s| ctx.intern_stmt(s)),
                    condition: condition.map(|e| ctx.intern_expr(e)),
                    post: post.map(|e| ctx.intern_expr(e)),
                    body: ctx.intern_stmt(body),
                    span: Span::empty(),
                    id: NodeId(0),
                }),
                span: Span::empty(),
                id: NodeId(0),
            }
        }
        pub fn exprstmt(expr: Expr) -> Stmt {
            let mut ctx = ctx();
            Stmt {
                kind: StmtKind::Expr(ctx.intern_expr(expr)),
                span: Span::empty(),
                id: NodeId(0),
            }
        }
        pub fn variable_decl(name: Ident, typ: Type, init: Option<Expr>) -> Stmt {
            let mut ctx = ctx();
            Stmt {
                kind: StmtKind::VariableDeclaration(VariableDeclaration {
                    is_extern: false,
                    var_type: typenode(typ, &mut ctx),
                    name,
                    init_value: init.map(|e| ctx.intern_expr(e)),
                    span: Span::empty(),
                    id: NodeId(0),
                }),
                span: Span::empty(),
                id: NodeId(0),
            }
        }
        pub fn func_decl(
            name: Ident,
            params: Vec<(Ident, Type)>,
            return_type: Option<Type>,
            body: Block,
        ) -> FunctionDeclaration {
            use utils::*;
            let mut ctx = ctx();
            FunctionDeclaration {
                inline: Inline::Auto,
                calling_convention: CallingConvention::Internal,
                return_type: return_type.map(|t| typenode(t, &mut ctx)),
                name,
                params: params
                    .into_iter()
                    .map(|(i, t)| (i, typenode(t, &mut ctx)))
                    .collect(),
                body,
                span: Span::empty(),
                id: NodeId(0),
            }
        }
        pub fn struct_decl(name: Ident, fields: Vec<(Ident, Type)>) -> StructDeclaration {
            use utils::*;
            let mut ctx = ctx();
            StructDeclaration {
                name,
                fields: fields
                    .into_iter()
                    .map(|(i, t)| (i, typenode(t, &mut ctx)))
                    .collect(),
                span: Span::empty(),
                id: NodeId(0),
            }
        }
    }

    #[test]
    fn test_number_parse() {
        #[track_caller]
        fn compare(s: &str, expected: i64) {
            use utils::*;
            let mut ctx = ctx();
            let lexed = lex(s, &mut ctx);
            let mut parser = Parser::new(&lexed);
            let atom = parser.parse_expr(&mut ctx).unwrap();
            let ExprKind::Int(Int { lit: parsed, .. }) = atom.kind else {
                unreachable!();
            };
            assert_eq!(parsed, expected)
        }
        #[track_caller]
        fn assert_fail(s: &str) {
            use utils::*;
            let mut ctx = ctx();
            let tokens = lex(s, &mut ctx);
            let mut parser = Parser::new(&tokens);
            let res = parser.parse_expr(&mut ctx).is_err();
            assert!(res || !parser.is_at_end() || !parser.errors.is_empty());
        }
        compare("9223372036854775807", 9223372036854775807);
        compare("0", 0);
        // Should be checked in ast validation since I couldn't fit it neatly
        // in the parser sadly...
        compare("9223372036854775808", -9223372036854775808);
        assert_fail("-9223372036854775809");
    }

    #[track_caller]
    fn compare_types(s: &str, expected: Type) {
        use utils::*;
        let mut ctx = ctx();
        let tokens = lex(s, &mut ctx);
        let mut parser = Parser::new(&tokens);
        let parsed = parser.parse_type(&mut ctx);
        let (typ, _span) = match parsed {
            Ok(t) => t,
            Err(_e) => {
                panic!("{s} did not parse properly");
            }
        };
        assert!(parser.is_at_end());
        assert!(parser.errors.is_empty());
        assert_eq!(typ, expected);
    }

    #[test]
    fn test_type_parsing_valid() {
        use utils::*;
        compare_types("void", void());
        compare_types("*void", ptr(void()));
        compare_types("***void", ptr(ptr(ptr(void()))));
        compare_types("int", int());
        compare_types("*int", ptr(int()));
        compare_types("***int", ptr(ptr(ptr(int()))));
        compare_types("[int; 1]", array(int(), 1));
        compare_types("[int; 0]", array(int(), 0));
        compare_types("[*int; 1]", array(ptr(int()), 1));
        compare_types("[[int; 1]; 1]", array(array(int(), 1), 1));
        compare_types("Something", struct_(ident("Something")));
        compare_types("[Something; 1]", array(struct_(ident("Something")), 1));
        compare_types("[int; 2555555]", array(int(), 2555555));
        compare_types("**Something", ptr(ptr(struct_(ident("Something")))));
        compare_types(
            "fn()",
            func_ptr(Vec::new(), Type::Void, FnPtrKind::Internal),
        );
        compare_types(
            "fn()->void",
            func_ptr(Vec::new(), Type::Void, FnPtrKind::Internal),
        );
        compare_types(
            "*fn()",
            ptr(func_ptr(Vec::new(), Type::Void, FnPtrKind::Internal)),
        );
        compare_types(
            "fn(int)",
            func_ptr(vec![int()], Type::Void, FnPtrKind::Internal),
        );
        compare_types("fn_sys()", func_ptr(Vec::new(), Type::Void, FnPtrKind::Abi));
        compare_types(
            "fn_sys()->void",
            func_ptr(Vec::new(), Type::Void, FnPtrKind::Abi),
        );
        compare_types(
            "*fn_sys()",
            ptr(func_ptr(Vec::new(), Type::Void, FnPtrKind::Abi)),
        );
        compare_types(
            "fn_sys(int)",
            func_ptr(vec![int()], Type::Void, FnPtrKind::Abi),
        );
        compare_types("noalias *int", noalias_ptr(int()));
        compare_types("noalias * noalias *int", noalias_ptr(noalias_ptr(int())));
        compare_types("noalias *void", noalias_ptr(void()));
        compare_types(
            "noalias *SomeStruct",
            noalias_ptr(struct_(ident("SomeStruct"))),
        );
        compare_types("noalias *[int; 255]", noalias_ptr(array(int(), 255)));
        compare_types("noalias **int", noalias_ptr(ptr(int())));
        compare_types("* noalias *int", ptr(noalias_ptr(int())));

        compare_types(
            "fn(int, int) -> int",
            func_ptr(vec![int(), int()], int(), FnPtrKind::Internal),
        );
        compare_types(
            "fn(int, int) -> SomeStruct",
            func_ptr(
                vec![int(), int()],
                struct_(ident("SomeStruct")),
                FnPtrKind::Internal,
            ),
        );
        compare_types(
            "fn(int, int) -> ***SomeStruct",
            func_ptr(
                vec![int(), int()],
                ptr(ptr(ptr(struct_(ident("SomeStruct"))))),
                FnPtrKind::Internal,
            ),
        );
        compare_types(
            "fn(SomeStruct, SomeOtherStruct) -> SomeStruct",
            func_ptr(
                vec![
                    struct_(ident("SomeStruct")),
                    struct_(ident("SomeOtherStruct")),
                ],
                struct_(ident("SomeStruct")),
                FnPtrKind::Internal,
            ),
        );
        compare_types(
            "fn(int, int) -> fn(int, int) -> int",
            func_ptr(
                vec![int(), int()],
                func_ptr(vec![int(), int()], int(), FnPtrKind::Internal),
                FnPtrKind::Internal,
            ),
        );
        compare_types(
            "fn(fn(int, int) -> fn(int, int) -> int, fn(fn(int, int) -> fn(int, int) -> int, int) -> fn(int, int) -> int) -> fn(int, int) -> int",
            func_ptr(
                vec![
                    func_ptr(
                        vec![int(), int()],
                        func_ptr(vec![int(), int()], int(), FnPtrKind::Internal),
                        FnPtrKind::Internal,
                    ),
                    func_ptr(
                        vec![
                            func_ptr(
                                vec![int(), int()],
                                func_ptr(vec![int(), int()], int(), FnPtrKind::Internal),
                                FnPtrKind::Internal,
                            ),
                            int(),
                        ],
                        func_ptr(vec![int(), int()], int(), FnPtrKind::Internal),
                        FnPtrKind::Internal,
                    ),
                ],
                func_ptr(vec![int(), int()], int(), FnPtrKind::Internal),
                FnPtrKind::Internal,
            ),
        );
    }

    #[test]
    fn test_type_parsing_fail() {
        use utils::*;
        let failing_tests = [
            "[int 10]",
            "[int;]",
            "[;10]",
            "fn int -> int",
            "fn() int",
            "fn() ->",
            "fn() -> fn int -> int",
            "fn() -> fn() int",
            "fn() -> fn() ->",
            "int*",
            "[int, 5]",
            "struct Something",
            "struct",
            "Something*",
        ];
        for s in failing_tests {
            let mut ctx = ctx();
            let tokens = lex(s, &mut ctx);
            let mut parser = Parser::new(&tokens);
            let res = parser.parse_type_node(&mut ctx).is_err();
            assert!(res || !parser.is_at_end() || !parser.errors.is_empty());
        }
    }

    #[track_caller]
    fn compare_exprs(s: &str, expected: Expr) {
        use utils::*;
        let mut ctx = ctx();
        let tokens = lex(s, &mut ctx);
        let mut parser = Parser::new(&tokens);
        let parsed = parser.parse_expr(&mut ctx).expect("Should parse correctly");
        assert!(parser.is_at_end());
        assert!(parser.errors.is_empty());
        assert!(parsed.ctx_eq(&expected, &ctx));
    }

    #[test]
    fn test_expr_parsing_valid() {
        use utils::*;
        compare_exprs("a << 1", shl(a(), num(1)));
        compare_exprs("a >> 1", shr(a(), num(1)));
        compare_exprs("a <<= 1", shleq(a(), num(1)));
        compare_exprs("a >>= 1", shreq(a(), num(1)));
        compare_exprs("a as int", cast(a(), get_type("int")));
        compare_exprs("a+b", add(a(), b()));
        compare_exprs("a-b", sub(a(), b()));
        compare_exprs("a*b", mul(a(), b()));
        compare_exprs("a/b", div(a(), b()));
        compare_exprs("a==b", eq(a(), b()));
        compare_exprs("a>b", gt(a(), b()));
        compare_exprs("a<b", lt(a(), b()));
        compare_exprs("a>=b", geq(a(), b()));
        compare_exprs("a<=b", leq(a(), b()));
        compare_exprs("a!=b", noteq(a(), b()));
        compare_exprs("a&&b", and(a(), b()));
        compare_exprs("a||b", or(a(), b()));
        compare_exprs("a&b", bitand(a(), b()));
        compare_exprs("a|b", bitor(a(), b()));
        compare_exprs("a^b", xor(a(), b()));
        compare_exprs("a%b", mod_(a(), b()));
        compare_exprs("a+=b", addeq(a(), b()));
        compare_exprs("a-=b", subeq(a(), b()));
        compare_exprs("a*=b", muleq(a(), b()));
        compare_exprs("a/=b", diveq(a(), b()));
        compare_exprs("a&=b", andeq(a(), b()));
        compare_exprs("a|=b", oreq(a(), b()));
        compare_exprs("a^=b", xoreq(a(), b()));
        compare_exprs("a%=b", modeq(a(), b()));
        compare_exprs("a=b", assign(a(), b()));
        compare_exprs("++a", prefix_increment(a()));
        compare_exprs("--a", prefix_decrement(a()));
        compare_exprs("+a", unary_plus(a()));
        compare_exprs("-a", unary_minus(a()));
        compare_exprs("&a", addr_of(a()));
        compare_exprs("*a", dereference(a()));
        compare_exprs("!a", not(a()));
        compare_exprs("a++", suffix_increment(a()));
        compare_exprs("a--", suffix_decrement(a()));
        compare_exprs("c ? a : b", ternary(c(), a(), b()));
        compare_exprs("c(a,b)", func_call(c(), vec![a(), b()]));
        compare_exprs("c.a", member_access(c(), ident("a")));
        compare_exprs("c->a", pointer_member_access(c(), ident("a")));
        compare_exprs("a[b]", array_index(a(), b()));
        compare_exprs("sizeof(int)", sizeof(get_type("int")));
        compare_exprs("sizeof(**int)", sizeof(get_type("**int")));
        compare_exprs("sizeof(a)", sizeof(get_type("a")));
        compare_exprs("*a", dereference(a()));
        compare_exprs("nullptr", nullptr());
        // Should fail in type checking, but parser should allow it
        compare_exprs("*nullptr", dereference(nullptr()));
        compare_exprs("~a", bitnot(a()));
        compare_exprs("~!!++a", bitnot(not(not(prefix_increment(a())))));
        compare_exprs("new_prov(nullptr)", new_prov(nullptr()));
        compare_exprs("copy_prov(nullptr, 15)", copy_prov(nullptr(), num(15)));
        compare_exprs("expose_prov(nullptr)", expose_prov(nullptr()));
        compare_exprs("unexpose_prov(15)", unexpose_prov(num(15)));
        compare_exprs(
            "unexpose_prov(expose_prov(unexpose_prov(nullptr)))",
            unexpose_prov(expose_prov(unexpose_prov(nullptr()))),
        );

        compare_exprs("(((((((((((a)))))))))))", a());
        compare_exprs("a as int", cast(a(), get_type("int")));
        compare_exprs("a(a,)", func_call(a(), vec![a()]));
        compare_exprs("a++ as int", cast(suffix_increment(a()), get_type("int")));
        compare_exprs("a + b * c", add(a(), mul(b(), c())));
        compare_exprs("a - b / c", sub(a(), div(b(), c())));
        compare_exprs("a && b || c", or(and(a(), b()), c()));
        compare_exprs("a + b == c", eq(add(a(), b()), c()));
        compare_exprs("a < b + c", lt(a(), add(b(), c())));
        compare_exprs("a = b + c", assign(a(), add(b(), c())));
        compare_exprs("a & b | c", bitor(bitand(a(), b()), c()));
        compare_exprs("a + (b * c)", add(a(), mul(b(), c())));
        compare_exprs("!a || b", or(not(a()), b()));
        compare_exprs("a == b != c", noteq(eq(a(), b()), c()));
        compare_exprs("++a * b", mul(prefix_increment(a()), b()));
        compare_exprs("--a + b", add(prefix_decrement(a()), b()));
        compare_exprs("*a + b", add(dereference(a()), b()));
        compare_exprs("a+++b", add(suffix_increment(a()), b()));
        compare_exprs("(a + b) * c", mul(add(a(), b()), c()));
        compare_exprs(
            "a ? b : c ? a : b",
            ternary(a(), b(), ternary(c(), a(), b())),
        );
        compare_exprs(
            "a ? b ? c : a : b",
            ternary(a(), ternary(b(), c(), a()), b()),
        );
        compare_exprs(
            "c(a + b, b * c)",
            func_call(c(), vec![add(a(), b()), mul(b(), c())]),
        );
        compare_exprs(
            "c.a + b->c",
            add(
                member_access(c(), ident("a")),
                pointer_member_access(b(), ident("c")),
            ),
        );
        compare_exprs("a[b + c]", array_index(a(), add(b(), c())));
        compare_exprs("sizeof(**int) + a", add(sizeof(get_type("**int")), a()));
        compare_exprs(
            "sizeof(int) * a + b",
            add(mul(sizeof(get_type("int")), a()), b()),
        );
        compare_exprs(
            "a + sizeof(int) * b",
            add(a(), mul(sizeof(get_type("int")), b())),
        );
        compare_exprs("a-->b", gt(suffix_decrement(a()), b()));

        compare_exprs("a + b * c - d", sub(add(a(), mul(b(), c())), d()));
        compare_exprs("a || b && c", or(a(), and(b(), c())));
        compare_exprs("a & b == c", bitand(a(), eq(b(), c())));
        compare_exprs("a + (b ? c : d)", add(a(), ternary(b(), c(), d())));
        compare_exprs("a = b += c * d", assign(a(), addeq(b(), mul(c(), d()))));
        compare_exprs("a + b % c", add(a(), mod_(b(), c())));
        compare_exprs("a ? b + c : d", ternary(a(), add(b(), c()), d()));
        compare_exprs("!a + b", add(not(a()), b()));
        compare_exprs("a & b == c | d", bitor(bitand(a(), eq(b(), c())), d()));
        compare_exprs("(a = b) == c", eq(assign(a(), b()), c()));
        compare_exprs(
            "++a * b--",
            mul(prefix_increment(a()), suffix_decrement(b())),
        );
        compare_exprs(
            "*a++ + *b",
            add(dereference(suffix_increment(a())), dereference(b())),
        );
        compare_exprs("&*a + b", add(addr_of(dereference(a())), b()));
        compare_exprs("a = *b++", assign(a(), dereference(suffix_increment(b()))));
        compare_exprs("a == (b = c)", eq(a(), assign(b(), c())));

        compare_exprs(
            "a + b * c == d && e || f",
            or(and(eq(add(a(), mul(b(), c())), d()), e()), f()),
        );
        compare_exprs("(a ? b : c) + d", add(ternary(a(), b(), c()), d()));
        compare_exprs(
            "a = b ? c + d : e * f",
            assign(a(), ternary(b(), add(c(), d()), mul(e(), f()))),
        );
        compare_exprs("a += b ? c : d", addeq(a(), ternary(b(), c(), d())));
        compare_exprs("a + (b += c)", add(a(), addeq(b(), c())));
        compare_exprs("a || b&& c == d", or(a(), and(b(), eq(c(), d()))));
        compare_exprs("a & b | c ^ d", bitor(bitand(a(), b()), xor(c(), d())));
        compare_exprs(
            "a = b | c & d ^ e",
            assign(a(), bitor(b(), xor(bitand(c(), d()), e()))),
        );
        compare_exprs(
            "a ? b ? c : d : e",
            ternary(a(), ternary(b(), c(), d()), e()),
        );
        compare_exprs("a = b = c + d", assign(a(), assign(b(), add(c(), d()))));
        compare_exprs("a + (b = c * d)", add(a(), assign(b(), mul(c(), d()))));
        compare_exprs(
            "a == b ? c++ :--d",
            ternary(eq(a(), b()), suffix_increment(c()), prefix_decrement(d())),
        );
        compare_exprs(
            "a + b ? c : d * e",
            ternary(add(a(), b()), c(), mul(d(), e())),
        );
        compare_exprs(
            "a++ + b++ * c",
            add(suffix_increment(a()), mul(suffix_increment(b()), c())),
        );
        compare_exprs("*(a + b) = c", assign(dereference(add(a(), b())), c()));
        compare_exprs("&a + *b", add(addr_of(a()), dereference(b())));
        compare_exprs(
            "a = (b && c) ? d : e",
            assign(a(), ternary(and(b(), c()), d(), e())),
        );
        compare_exprs("!a && b || c", or(and(not(a()), b()), c()));

        compare_exprs(
            "a = b + (c ? d * e : f / g) - a",
            assign(
                a(),
                sub(add(b(), ternary(c(), mul(d(), e()), div(f(), g()))), a()),
            ),
        );
        compare_exprs(
            "a ? b ? c : d : e ? f : g",
            ternary(a(), ternary(b(), c(), d()), ternary(e(), f(), g())),
        );
        compare_exprs(
            "a =b ? c = d + e : f * g",
            assign(a(), ternary(b(), assign(c(), add(d(), e())), mul(f(), g()))),
        );
        compare_exprs(
            "a += b || c && d ? e + f : g - g",
            addeq(
                a(),
                ternary(or(b(), and(c(), d())), add(e(), f()), sub(g(), g())),
            ),
        );
        compare_exprs("a = b = c = d", assign(a(), assign(b(), assign(c(), d()))));
        compare_exprs(
            "*a++ = *b++ + c",
            assign(
                dereference(suffix_increment(a())),
                add(dereference(suffix_increment(b())), c()),
            ),
        );
        compare_exprs(
            "a = *b++ ? *c-- : d",
            assign(
                a(),
                ternary(
                    dereference(suffix_increment(b())),
                    dereference(suffix_decrement(c())),
                    d(),
                ),
            ),
        );
        compare_exprs("!(a + b * (c - d))", not(add(a(), mul(b(), sub(c(), d())))));
        compare_exprs(
            "a++ * --b + (c ? d++ : e--)",
            add(
                mul(suffix_increment(a()), prefix_decrement(b())),
                ternary(c(), suffix_increment(d()), suffix_decrement(e())),
            ),
        );
        compare_exprs(
            "*(a = b + c) += d",
            addeq(dereference(assign(a(), add(b(), c()))), d()),
        );
        compare_exprs("&(a ? b : c)", addr_of(ternary(a(), b(), c())));
        compare_exprs(
            "a = (b ? c : d) ? e : f",
            assign(a(), ternary(ternary(b(), c(), d()), e(), f())),
        );
        compare_exprs(
            "a ? b || c && d : e + f * g",
            ternary(a(), or(b(), and(c(), d())), add(e(), mul(f(), g()))),
        );
        compare_exprs(
            "a = b + (c = d * (e + f))",
            assign(a(), add(b(), assign(c(), mul(d(), add(e(), f()))))),
        );
        compare_exprs(
            "(* a)[b] = a ? g[d] : f",
            assign(
                array_index(dereference(a()), b()),
                ternary(a(), array_index(g(), d()), f()),
            ),
        );
        compare_exprs(
            "** a = (*b)[c] + d",
            assign(
                dereference(dereference(a())),
                add(array_index(dereference(b()), c()), d()),
            ),
        );
        compare_exprs(
            "a=b?c?d:e:f?g:g",
            assign(
                a(),
                ternary(b(), ternary(c(), d(), e()), ternary(f(), g(), g())),
            ),
        );

        compare_exprs(
            "*++ *a = a ? ((&((a++ + b)[c] * (--b *= (c ? ++d : (e -- ? *f++ : g[b]))))) && g-- ? * f++ : d[b]) : e",
            assign(
                dereference(prefix_increment(dereference(a()))),
                ternary(
                    a(),
                    ternary(
                        and(
                            addr_of(mul(
                                array_index(add(suffix_increment(a()), b()), c()),
                                muleq(
                                    prefix_decrement(b()),
                                    ternary(
                                        c(),
                                        prefix_increment(d()),
                                        ternary(
                                            suffix_decrement(e()),
                                            dereference(suffix_increment(f())),
                                            array_index(g(), b()),
                                        ),
                                    ),
                                ),
                            )),
                            suffix_decrement(g()),
                        ),
                        dereference(suffix_increment(f())),
                        array_index(d(), b()),
                    ),
                    e(),
                ),
            ),
        );
        compare_exprs(
            "*(&a) = !(*(a++ * --b + (c ? d:e) -f--) += a ? *b++ : &c ) ?(b = c+d) : (c ? d : e)",
            assign(
                dereference(addr_of(a())),
                ternary(
                    not(addeq(
                        dereference(sub(
                            add(
                                mul(suffix_increment(a()), prefix_decrement(b())),
                                ternary(c(), d(), e()),
                            ),
                            suffix_decrement(f()),
                        )),
                        ternary(a(), dereference(suffix_increment(b())), addr_of(c())),
                    )),
                    assign(b(), add(c(), d())),
                    ternary(c(), d(), e()),
                ),
            ),
        );
        compare_exprs(
            "!((a[*( (a++ * b) % g[a + b] + (c ? d : e ? sizeof(int) * f : g) - (b->c as int) )] *= c(a + b, b * c)) as int)",
            not(cast(
                muleq(
                    array_index(
                        a(),
                        dereference(sub(
                            add(
                                mod_(
                                    mul(suffix_increment(a()), b()),
                                    array_index(g(), add(a(), b())),
                                ),
                                ternary(
                                    c(),
                                    d(),
                                    ternary(e(), mul(sizeof(get_type("int")), f()), g()),
                                ),
                            ),
                            cast(pointer_member_access(b(), ident("c")), get_type("int")),
                        )),
                    ),
                    func_call(c(), vec![add(a(), b()), mul(b(), c())]),
                ),
                get_type("int"),
            )),
        );
    }

    #[test]
    fn test_expr_parsing_fail() {
        use utils::*;
        let failing_tests = [
            "a + ",
            "a * ",
            "a / ",
            "a - ",
            "a == ",
            "a != ",
            "a > ",
            "a < ",
            "a >= ",
            "a <= ",
            "a && ",
            "a || ",
            "a & ",
            "a | ",
            "a ^ ",
            "a % ",
            "a += ",
            "a -= ",
            "a *= ",
            "a /= ",
            "a &= ",
            "a |= ",
            "a ^= ",
            "a %= ",
            "a = ",
            "++",
            "--",
            "+",
            "-",
            "&",
            "*",
            "!",
            "a ? b :",
            "a ? : b",
            "a ? b",
            "c(, a)",
            "c(.a)",
            "c(->a)",
            ".a",
            "->a",
            "a[]",
            "a[ ]",
            "sizeof()",
            "sizeof int",
            "sizeof(int*)",
            "expose_prov()",
            "expose_prov(a, b)",
            "new_prov()",
            "new_prov(a, b)",
            "copy_prov()",
            "copy_prov(a)",
            "copy_prov(a, b, c)",
        ];
        for s in failing_tests {
            let mut ctx = ctx();
            let lexed = lex(s, &mut ctx);
            let mut parser = Parser::new(&lexed);
            assert!(
                parser.parse_expr(&mut ctx).is_err()
                    || !parser.is_at_end()
                    || !parser.errors.is_empty()
            );
        }
    }

    #[track_caller]
    fn compare_struct_init(s: &str, expected: Expr) {
        use utils::*;
        let mut ctx = ctx();
        let lexed = lex(s, &mut ctx);
        let mut parser = Parser::new(&lexed);
        let parsed = parser.parse_struct_init(&mut ctx).unwrap();
        assert!(parser.is_at_end());
        assert!(parser.errors.is_empty());
        assert!(parsed.ctx_eq(&expected, &ctx));
    }

    #[test]
    fn test_struct_init_valid() {
        use utils::*;
        compare_struct_init("MyStruct {}", struct_init(ident("MyStruct"), vec![]));
        compare_struct_init(
            "MyStruct {a: b}",
            struct_init(ident("MyStruct"), vec![(ident("a"), b())]),
        );
        compare_struct_init(
            "MyStruct {a: b,}",
            struct_init(ident("MyStruct"), vec![(ident("a"), b())]),
        );
        compare_struct_init(
            "MyStruct {a: b, c: d}",
            struct_init(
                ident("MyStruct"),
                vec![(ident("a"), b()), (ident("c"), d())],
            ),
        );
        compare_struct_init(
            "MyStruct {a: b+c, d: e?f:g}",
            struct_init(
                ident("MyStruct"),
                vec![
                    (ident("a"), add(b(), c())),
                    (ident("d"), ternary(e(), f(), g())),
                ],
            ),
        );
    }

    #[test]
    fn test_struct_init_invalid() {
        use utils::*;
        let fails = [
            "MyStruct {",
            "MyStruct {a:}",
            "MyStruct {a b}",
            "MyStruct {a: b c: d}",
            "MyStruct {a: b, c d}",
            "MyStruct {a: b, c:}",
            "MyStruct {a: b, : d}",
            "MyStruct {a: b, c: d,,}",
            "MyStruct {a: b, c: d e: f}",
            "struct MyStruct {a: b, c: d}",
            "struct {a: b, c:d}",
            "struct MyStruct a: b, c: d}",
        ];
        for s in fails {
            let mut ctx = ctx();
            let lexed = lex(s, &mut ctx);
            let mut parser = Parser::new(&lexed);
            assert!(
                parser.parse_struct_init(&mut ctx).is_err()
                    || !parser.is_at_end()
                    || !parser.errors.is_empty()
            );
        }
    }

    #[track_caller]
    fn compare_array_init(s: &str, expected: Expr) {
        use utils::*;
        let mut ctx = ctx();
        let lexed = lex(s, &mut ctx);
        let mut parser = Parser::new(&lexed);
        let parsed = parser.parse_array_init(&mut ctx).unwrap();
        assert!(parser.is_at_end());
        assert!(parser.errors.is_empty());
        assert!(parsed.ctx_eq(&expected, &ctx));
    }

    #[test]
    fn test_array_init_valid() {
        use utils::*;
        compare_array_init("[]", array_init(vec![]));
        compare_array_init("[a]", array_init(vec![a()]));
        compare_array_init("[a, b, c, d, e]", array_init(vec![a(), b(), c(), d(), e()]));
        compare_array_init(
            "[a ? b : c, c+d, d+e, f->d]",
            array_init(vec![
                ternary(a(), b(), c()),
                add(c(), d()),
                add(d(), e()),
                pointer_member_access(f(), ident("d")),
            ]),
        );
        compare_array_init(
            "[[a, b], [c, d]]",
            array_init(vec![array_init(vec![a(), b()]), array_init(vec![c(), d()])]),
        );
        compare_array_init(
            "[[[[a]]]]",
            array_init(vec![array_init(vec![array_init(vec![array_init(vec![
                a(),
            ])])])]),
        );
        compare_array_init(
            "[MyStruct {a: b}, MyStruct {d: e}]",
            array_init(vec![
                struct_init(ident("MyStruct"), vec![(ident("a"), b())]),
                struct_init(ident("MyStruct"), vec![(ident("d"), e())]),
            ]),
        );
    }

    #[test]
    fn test_array_init_invalid() {
        use utils::*;
        let fails = ["[0; 1]", "[0 1]", "[0, 1}", "[}"];
        for s in fails {
            let mut ctx = ctx();
            let lexed = lex(s, &mut ctx);
            let mut parser = Parser::new(&lexed);
            assert!(
                parser.parse_array_init(&mut ctx).is_err()
                    || !parser.is_at_end()
                    || !parser.errors.is_empty()
            );
        }
    }

    #[track_caller]
    fn compare_return(s: &str, expected: Stmt) {
        use utils::*;
        let mut ctx = ctx();
        let lexed = lex(s, &mut ctx);
        let mut parser = Parser::new(&lexed);
        let parsed = parser.parse_return_stmt(&mut ctx).unwrap();
        assert!(parser.is_at_end());
        assert!(parser.errors.is_empty());
        assert!(parsed.ctx_eq(&expected, &ctx));
    }

    #[test]
    fn test_return_stmt_valid() {
        use utils::*;
        compare_return("return;", return_stmt(None));
        compare_return("return a;", return_stmt(Some(a())));
        compare_return("return a+b;", return_stmt(Some(add(a(), b()))));
        compare_return(
            "return a ? b : c;",
            return_stmt(Some(ternary(a(), b(), c()))),
        );
        compare_return(
            "return Something {a: b};",
            return_stmt(Some(struct_init(
                ident("Something"),
                vec![(ident("a"), b())],
            ))),
        );
    }

    #[track_caller]
    fn compare_while(s: &str, expected: Stmt) {
        use utils::*;
        let mut ctx = ctx();
        let lexed = lex(s, &mut ctx);
        let mut parser = Parser::new(&lexed);
        let parsed = parser.parse_while_loop(&mut ctx).unwrap();
        assert!(parser.is_at_end());
        assert!(parser.errors.is_empty());
        assert!(parsed.ctx_eq(&expected, &ctx));
    }

    #[test]
    fn test_while_stmt_valid() {
        use utils::*;
        compare_while("while (a) {}", while_loop(a(), block(vec![])));
        compare_while("while (a) b;", while_loop(a(), exprstmt(b())));
        compare_while("while (a) return;", while_loop(a(), return_stmt(None)));
        compare_while(
            "while (b) {return;}",
            while_loop(b(), block(vec![return_stmt(None)])),
        );
    }

    #[test]
    fn test_while_stmt_invalid() {
        use utils::*;
        let fails = [
            "while a {}",
            "while(){}",
            "while a) {}",
            "while a (a) {}",
            "while (a {}",
            "while (a) ",
            "while (a) return",
            "while ()",
        ];
        for s in fails {
            let mut ctx = ctx();
            let lexed = lex(s, &mut ctx);
            let mut parser = Parser::new(&lexed);
            assert!(
                parser.parse_while_loop(&mut ctx).is_err()
                    || !parser.is_at_end()
                    || !parser.errors.is_empty()
            );
        }
    }

    #[track_caller]
    fn compare_block(s: &str, expected: Stmt) {
        use utils::*;
        let mut ctx = ctx();
        let lexed = lex(s, &mut ctx);
        let mut parser = Parser::new(&lexed);
        let parsed = Stmt {
            kind: StmtKind::Block(parser.parse_block(&mut ctx).unwrap()),
            span: Span::empty(),
            id: NodeId(0),
        };
        assert!(parser.is_at_end());
        assert!(parser.errors.is_empty());
        assert!(parsed.ctx_eq(&expected, &ctx));
    }

    #[test]
    fn test_block_stmt_valid() {
        use utils::*;
        compare_block("{}", block(vec![]));
        compare_block("{return;}", block(vec![return_stmt(None)]));
        compare_block("{return a;}", block(vec![return_stmt(Some(a()))]));
        compare_block(
            "{return a+b;}",
            block(vec![return_stmt(Some(add(a(), b())))]),
        );
        compare_block(
            "{return a; return b;}",
            block(vec![return_stmt(Some(a())), return_stmt(Some(b()))]),
        );
        compare_block(
            "{return; return a; return a+b;}",
            block(vec![
                return_stmt(None),
                return_stmt(Some(a())),
                return_stmt(Some(add(a(), b()))),
            ]),
        );
        compare_block(
            "{{{{}}}}",
            block(vec![block(vec![block(vec![block(vec![])])])]),
        );
    }

    #[test]
    fn test_block_stmt_invalid() {
        use utils::*;
        let fails = [
            "{",
            "{return",
            "{return;",
            "{return a",
            "{return a;",
            "{return a+b",
            "{return a+b;",
            "{return; return",
            "{return; return a",
            "{return; return a;",
            "{{}",
        ];
        for s in fails {
            let mut ctx = ctx();
            let lexed = lex(s, &mut ctx);
            let mut parser = Parser::new(&lexed);
            assert!(
                parser.parse_block(&mut ctx).is_err()
                    || !parser.is_at_end()
                    || !parser.errors.is_empty()
            );
        }
    }

    #[track_caller]
    fn compare_if(s: &str, expected: Stmt) {
        use utils::*;
        let mut ctx = ctx();
        let lexed = lex(s, &mut ctx);
        let mut parser = Parser::new(&lexed);
        let parsed = parser.parse_if_stmt(&mut ctx).unwrap();
        assert!(parser.is_at_end());
        assert!(parser.errors.is_empty());
        assert!(parsed.ctx_eq(&expected, &ctx));
    }

    #[test]
    fn test_if_stmt_success() {
        use utils::*;
        compare_if("if (a) b;", if_stmt(a(), exprstmt(b()), None));
        compare_if(
            "if (a) b; else c;",
            if_stmt(a(), exprstmt(b()), Some(exprstmt(c()))),
        );
        compare_if(
            "if (a) b; else if (c) d; else e;",
            if_stmt(
                a(),
                exprstmt(b()),
                Some(if_stmt(c(), exprstmt(d()), Some(exprstmt(e())))),
            ),
        );
        compare_if("if (a) {}", if_stmt(a(), block(vec![]), None));
        compare_if(
            "if (a) {} else {}",
            if_stmt(a(), block(vec![]), Some(block(vec![]))),
        );
        compare_if(
            "if (a) {a;} else {a;}",
            if_stmt(
                a(),
                block(vec![exprstmt(a())]),
                Some(block(vec![exprstmt(a())])),
            ),
        );
        compare_if("if(a)b;", if_stmt(a(), exprstmt(b()), None));
        compare_if("if ( a ) b;", if_stmt(a(), exprstmt(b()), None));
        compare_if("if (a)\n b;", if_stmt(a(), exprstmt(b()), None));
        compare_if(
            "if\n\n\n\n(\na\n\n)\n{\n\na;\n}\nelse\n{\nb;\n}",
            if_stmt(
                a(),
                block(vec![exprstmt(a())]),
                Some(block(vec![exprstmt(b())])),
            ),
        );
        compare_if(
            "if (a) if (b) c;",
            if_stmt(a(), if_stmt(b(), exprstmt(c()), None), None),
        );
        compare_if(
            "if (a) { if (b) c; else d; }",
            if_stmt(
                a(),
                block(vec![if_stmt(b(), exprstmt(c()), Some(exprstmt(d())))]),
                None,
            ),
        );
        compare_if(
            "if (a ? b : c++) d; else e++;",
            if_stmt(
                ternary(a(), b(), suffix_increment(c())),
                exprstmt(d()),
                Some(exprstmt(suffix_increment(e()))),
            ),
        );
        compare_if(
            "if (a) if (b) e; else c; else d;",
            if_stmt(
                a(),
                if_stmt(b(), exprstmt(e()), Some(exprstmt(c()))),
                Some(exprstmt(d())),
            ),
        )
    }

    #[test]
    fn test_if_stmt_invalid() {
        use utils::*;
        let fails = [
            "if a b;",
            "if (a b;",
            "if (a) b",
            "if (a) else b;",
            "if (a) b; else",
            "if (a) b else c;",
            "if (a) b; else if c d;",
            "if (a) {a}",
            "if (a)",
            "if (a) else",
            "if (a)) b;",
            "if ((a) b;",
            "if () b;",
            "if (a) ; else;",
            "if (a) { else b; }",
            "if (a) {b;} else else {c;}",
        ];
        for s in fails {
            let mut ctx = ctx();
            let lexed = lex(s, &mut ctx);
            let mut parser = Parser::new(&lexed);
            assert!(
                parser.parse_if_stmt(&mut ctx).is_err()
                    || !parser.is_at_end()
                    || !parser.errors.is_empty()
            );
        }
    }

    #[track_caller]
    fn compare_for(s: &str, expected: Stmt) {
        use utils::*;
        let mut ctx = ctx();
        let lexed = lex(s, &mut ctx);
        let mut parser = Parser::new(&lexed);
        let parsed = parser.parse_for_loop(&mut ctx).unwrap();
        assert!(parser.is_at_end());
        assert!(parser.errors.is_empty());
        assert!(parsed.ctx_eq(&expected, &ctx));
    }

    #[test]
    fn test_for_loop_valid() {
        use utils::*;
        compare_for("for (;;) {}", for_loop(None, None, None, block(vec![])));
        compare_for(
            "for (;;) return;",
            for_loop(None, None, None, return_stmt(None)),
        );
        compare_for(
            "for (;;) for (;;) a;",
            for_loop(None, None, None, for_loop(None, None, None, exprstmt(a()))),
        );
        compare_for(
            "for (let i: int = 0; i < 10; i++) {}",
            for_loop(
                Some(variable_decl(ident("i"), get_type("int"), Some(num(0)))),
                Some(lt(i(), num(10))),
                Some(suffix_increment(i())),
                block(vec![]),
            ),
        );
        compare_for(
            "for (let i: MyStruct = MyStruct {};;) a;",
            for_loop(
                Some(variable_decl(
                    ident("i"),
                    get_type("MyStruct"),
                    Some(struct_init(ident("MyStruct"), vec![])),
                )),
                None,
                None,
                exprstmt(a()),
            ),
        );

        compare_for(
            "for (i = 5; i < 10; i += 2) { return i; }",
            for_loop(
                Some(exprstmt(assign(i(), num(5)))),
                Some(lt(i(), num(10))),
                Some(addeq(i(), num(2))),
                block(vec![return_stmt(Some(i()))]),
            ),
        );
        compare_for(
            "for (;i != 10;) a = a + 1;",
            for_loop(
                None,
                Some(noteq(i(), num(10))),
                None,
                exprstmt(assign(a(), add(a(), num(1)))),
            ),
        );
        compare_for(
            "for (;;i = i * 2) {}",
            for_loop(
                None,
                None,
                Some(assign(i(), mul(i(), num(2)))),
                block(vec![]),
            ),
        );
        compare_for(
            "for (let i: int = 0;; i++) for (let a: int = 10; a > 0; a--) {}",
            for_loop(
                Some(variable_decl(ident("i"), get_type("int"), Some(num(0)))),
                None,
                Some(suffix_increment(i())),
                for_loop(
                    Some(variable_decl(ident("a"), get_type("int"), Some(num(10)))),
                    Some(gt(a(), num(0))),
                    Some(suffix_decrement(a())),
                    block(vec![]),
                ),
            ),
        );
    }

    #[test]
    fn test_for_loop_invalid() {
        use utils::*;
        let fails = [
            "for );;) {}",
            "for (let i: int = 0; i < 10 i++) {}",
            "for (let i: int = 0 i < 10; i++) {}",
            "for (i = 0; i < 10; i++)",
            "for (let i: int = ; i < 10; i++) {}",
            "for ({};;) {}",
        ];
        for s in fails {
            let mut ctx = ctx();
            let lexed = lex(s, &mut ctx);
            let mut parser = Parser::new(&lexed);
            assert!(
                parser.parse_for_loop(&mut ctx).is_err()
                    || !parser.is_at_end()
                    || !parser.errors.is_empty()
            );
        }
    }

    #[track_caller]
    fn compare_vardecl(s: &str, expected: Stmt) {
        use utils::*;
        let mut ctx = ctx();
        let lexed = lex(s, &mut ctx);
        let mut parser = Parser::new(&lexed);
        let parsed = Stmt {
            kind: StmtKind::VariableDeclaration(parser.parse_variable_decl(&mut ctx, &[]).unwrap()),
            span: Span::empty(),
            id: NodeId(0),
        };
        assert!(parser.is_at_end());
        assert!(parser.errors.is_empty());
        assert!(parsed.ctx_eq(&expected, &ctx));
    }

    #[test]
    fn test_vardecl_valid() {
        use utils::*;
        compare_vardecl(
            "let a: int",
            variable_decl(ident("a"), get_type("int"), None),
        );
        compare_vardecl(
            "let a: *int",
            variable_decl(ident("a"), get_type("*int"), None),
        );
        compare_vardecl(
            "let b: MyStruct",
            variable_decl(ident("b"), get_type("MyStruct"), None),
        );
        compare_vardecl(
            "let c: [int; 10]",
            variable_decl(ident("c"), get_type("[int; 10]"), None),
        );
        compare_vardecl(
            "let a: int = a",
            variable_decl(ident("a"), get_type("int"), Some(a())),
        );
        compare_vardecl(
            "let a: int = a ? b : c++ + d * e",
            variable_decl(
                ident("a"),
                get_type("int"),
                Some(ternary(a(), b(), add(suffix_increment(c()), mul(d(), e())))),
            ),
        );
        compare_vardecl(
            "let a: MyStruct = MyStruct { a: b, c: d}",
            variable_decl(
                ident("a"),
                get_type("MyStruct"),
                Some(struct_init(
                    ident("MyStruct"),
                    vec![(ident("a"), b()), (ident("c"), d())],
                )),
            ),
        );
        compare_vardecl(
            "let a: [int; 5] = [b, c, d, e, f]",
            variable_decl(
                ident("a"),
                get_type("[int; 5]"),
                Some(array_init(vec![b(), c(), d(), e(), f()])),
            ),
        );
        compare_vardecl(
            "let a: *int = &b",
            variable_decl(ident("a"), get_type("*int"), Some(addr_of(b()))),
        );
        compare_vardecl(
            "let a: *void",
            variable_decl(ident("a"), get_type("*void"), None),
        );
        compare_vardecl(
            "let b: *void = nullptr",
            variable_decl(ident("b"), get_type("*void"), Some(nullptr())),
        );
    }

    #[test]
    fn test_vardecl_invalid() {
        use utils::*;
        let fails = [
            "let a int",
            "let a: int=",
            "let a: int = ",
            "let a: = b",
            "let : int = b",
            "let a int = b",
            "let a: struct MyStruct = struct MyStruct { a: b, c d}",
            "let a: [int; 5] = [b, c, d e]",
            "let a = 5",
            "let c: [int] = [a, b, c]",
            "let int: int",
            "let a: void* = nullptr",
        ];
        for s in fails {
            let mut ctx = ctx();
            let lexed = lex(s, &mut ctx);
            let mut parser = Parser::new(&lexed);
            assert!(
                parser.parse_variable_decl(&mut ctx, &[]).is_err()
                    || !parser.is_at_end()
                    || !parser.errors.is_empty()
            );
        }
    }

    #[track_caller]
    fn compare_funcdecl(s: &str, expected: FunctionDeclaration) {
        use utils::*;
        let mut ctx = ctx();
        let lexed = lex(s, &mut ctx);
        let mut parser = Parser::new(&lexed);
        let parsed = parser.parse_function_decl(&mut ctx, &[]).unwrap();
        assert!(parser.is_at_end());
        assert!(parser.errors.is_empty());
        assert!(parsed.ctx_eq(&expected, &ctx));
    }

    #[test]
    fn test_funcdecl_valid() {
        use utils::*;
        compare_funcdecl(
            "fn f() {}",
            func_decl(ident("f"), vec![], None, bblock(vec![])),
        );
        compare_funcdecl(
            "fn f() -> void {}",
            func_decl(ident("f"), vec![], Some(get_type("void")), bblock(vec![])),
        );
        compare_funcdecl(
            "fn f() -> int {}",
            func_decl(ident("f"), vec![], Some(get_type("int")), bblock(vec![])),
        );
        compare_funcdecl(
            "fn f() -> **int{}",
            func_decl(ident("f"), vec![], Some(get_type("**int")), bblock(vec![])),
        );
        compare_funcdecl(
            "fn f(a: int, b: *MyStruct) -> **MyStruct {}",
            func_decl(
                ident("f"),
                vec![
                    (ident("a"), get_type("int")),
                    (ident("b"), get_type("*MyStruct")),
                ],
                Some(get_type("**MyStruct")),
                bblock(vec![]),
            ),
        );
        compare_funcdecl(
            "fn g(a: *MyStruct, b: fn(int) -> int) -> fn(*Struct) -> **Struct { return; }",
            func_decl(
                ident("g"),
                vec![
                    (ident("a"), get_type("*MyStruct")),
                    (ident("b"), get_type("fn(int) -> int")),
                ],
                Some(get_type("fn(*Struct) -> **Struct")),
                bblock(vec![return_stmt(None)]),
            ),
        );
    }

    #[test]
    fn test_funcdecl_invalid() {
        use utils::*;
        let fails = [
            "fn f()",
            "fn f() -> int",
            "fn f(a: int, b: int)",
            "fn f() return",
            "fn f(int a) {}",
            "fn f(a int) {}",
            "fn f(a: int b: int) {}",
            "fn f(a: int, b int) {}",
        ];
        for s in fails {
            let mut ctx = ctx();
            let lexed = lex(s, &mut ctx);
            let mut parser = Parser::new(&lexed);
            assert!(
                parser.parse_function_decl(&mut ctx, &[]).is_err()
                    || !parser.is_at_end()
                    || !parser.errors.is_empty()
            );
        }
    }

    #[track_caller]
    fn compare_structdecl(s: &str, expected: StructDeclaration) {
        use utils::*;
        let mut ctx = ctx();
        let lexed = lex(s, &mut ctx);
        let mut parser = Parser::new(&lexed);
        let parsed = parser.parse_struct_decl(&mut ctx, &[]).unwrap();
        assert!(parser.is_at_end());
        assert!(parser.errors.is_empty());
        assert!(parsed.ctx_eq(&expected, &ctx));
    }

    #[test]
    fn test_structdecl_valid() {
        use utils::*;
        compare_structdecl("struct MyStruct {}", struct_decl(ident("MyStruct"), vec![]));
        compare_structdecl(
            "struct MyStruct {a: int}",
            struct_decl(ident("MyStruct"), vec![(ident("a"), get_type("int"))]),
        );
        compare_structdecl(
            "struct MyStruct {a: int,}",
            struct_decl(ident("MyStruct"), vec![(ident("a"), get_type("int"))]),
        );
        compare_structdecl(
            "struct MyStruct {b: OtherStruct, c: *OtherStruct}",
            struct_decl(
                ident("MyStruct"),
                vec![
                    (ident("b"), get_type("OtherStruct")),
                    (ident("c"), get_type("*OtherStruct")),
                ],
            ),
        );
        compare_structdecl(
            "struct MyStruct { b: *fn(int) -> int, c: [int; 10]}",
            struct_decl(
                ident("MyStruct"),
                vec![
                    (ident("b"), get_type("*fn(int) -> int")),
                    (ident("c"), get_type("[int; 10]")),
                ],
            ),
        );
    }

    #[test]
    fn test_structdecl_invalid() {
        use utils::*;
        let fails = [
            "struct MyStruct (",
            "struct MyStruct [",
            "struct MyStruct {",
            "struct MyStruct { a: int b: int }",
            "struct MyStruct { a: int, b int }",
            "struct MyStruct { a: int, b: }",
            "struct MyStruct { : int }",
            "struct MyStruct {int a}",
        ];
        for s in fails {
            let mut ctx = ctx();
            let lexed = lex(s, &mut ctx);
            let mut parser = Parser::new(&lexed);
            assert!(
                parser.parse_struct_decl(&mut ctx, &[]).is_err()
                    || !parser.is_at_end()
                    || !parser.errors.is_empty()
            );
        }
    }

    #[track_caller]
    fn compare_stmt(s: &str, expected: Stmt) {
        use utils::*;
        let mut ctx = ctx();
        let lexed = lex(s, &mut ctx);
        let mut parser = Parser::new(&lexed);
        let parsed = parser.parse_stmt(&mut ctx).unwrap();
        assert!(parser.is_at_end());
        assert!(parser.errors.is_empty());
        assert!(parsed.ctx_eq(&expected, &ctx));
    }

    #[test]
    fn test_stmt_parsing_valid() {
        use utils::*;
        compare_stmt("return a + b;", return_stmt(Some(add(a(), b()))));
        compare_stmt("while (a) b;", while_loop(a(), exprstmt(b())));
        compare_stmt("{return;}", block(vec![return_stmt(None)]));
        compare_stmt(
            "if (a) b; else c;",
            if_stmt(a(), exprstmt(b()), Some(exprstmt(c()))),
        );
        compare_stmt(
            "for (;;) return;",
            for_loop(None, None, None, return_stmt(None)),
        );
        compare_stmt(
            "let a: int = b + c;",
            variable_decl(ident("a"), get_type("int"), Some(add(b(), c()))),
        );
        compare_stmt("continue;", continue_());
        compare_stmt("break;", break_());
        compare_stmt("a += 5;", exprstmt(addeq(a(), num(5))));
    }

    #[test]
    fn test_stmt_invalid() {
        use utils::*;
        let fails = [
            "return a + b",
            "while (a) b",
            "{return; ",
            "if (a) b; else",
            "for (;;) return",
            "let a: int = b + c",
            "continue",
            "break",
            "a += 5",
            "for (let i: int = 0; i < 10; i++) something",
        ];
        for s in fails {
            let mut ctx = ctx();
            let lexed = lex(s, &mut ctx);
            let mut parser = Parser::new(&lexed);
            assert!(
                parser.parse_stmt(&mut ctx).is_err()
                    || !parser.is_at_end()
                    || !parser.errors.is_empty()
            );
        }
    }

    #[track_caller]
    fn compare_global_decl(s: &str, expected: GlobalDeclaration) {
        use utils::*;
        let mut ctx = ctx();
        let lexed = lex(s, &mut ctx);
        let mut parser = Parser::new(&lexed);
        let parsed = parser.parse_global_decl(&mut ctx).unwrap();
        assert!(parser.is_at_end());
        assert!(parser.errors.is_empty());
        assert!(parsed.ctx_eq(&expected, &ctx));
    }

    #[test]
    fn test_global_decl() {
        use utils::*;
        compare_global_decl(
            "fn f() {}",
            GlobalDeclaration {
                kind: GlobalDeclarationKind::Function(func_decl(
                    ident("f"),
                    vec![],
                    None,
                    bblock(vec![]),
                )),
                span: Span::empty(),
                id: NodeId(0),
            },
        );
        compare_global_decl(
            "struct MyStruct { a: int, b: *int }",
            GlobalDeclaration {
                kind: GlobalDeclarationKind::Struct(struct_decl(
                    ident("MyStruct"),
                    vec![
                        (ident("a"), get_type("int")),
                        (ident("b"), get_type("*int")),
                    ],
                )),
                span: Span::empty(),
                id: NodeId(0),
            },
        );
        let typ = get_type("int");
        let ident = ident("a");
        let mut ctx = ctx();
        let typenode = typenode(typ, &mut ctx);
        let init_value = Some(ctx.intern_expr(num(5)));
        drop(ctx);
        compare_global_decl(
            "let a: int = 5;",
            GlobalDeclaration {
                kind: GlobalDeclarationKind::Variable(VariableDeclaration {
                    is_extern: false,
                    var_type: typenode,
                    name: ident,
                    init_value,
                    span: Span::empty(),
                    id: NodeId(0),
                }),
                span: Span::empty(),
                id: NodeId(0),
            },
        );
        #[track_caller]
        fn compile_global_decl(s: &str) -> GlobalDeclaration {
            use utils::*;
            let mut ctx = ctx();
            let lexed = lex(s, &mut ctx);
            let mut parser = Parser::new(&lexed);
            parser.parse_global_decl(&mut ctx).unwrap()
        }
        #[track_caller]
        fn compile_func_decl(s: &str) -> FunctionDeclaration {
            compile_global_decl(s).kind.to_function_decl().unwrap()
        }
        #[track_caller]
        fn compile_vardecl(s: &str) -> VariableDeclaration {
            compile_global_decl(s).kind.to_variable_decl().unwrap()
        }
        #[track_caller]
        fn compile_structdecl(s: &str) -> StructDeclaration {
            compile_global_decl(s).kind.to_struct_decl().unwrap()
        }
        let func1 = compile_func_decl("@[] fn foo() {}");
        assert!(func1.inline == Inline::Auto);
        assert!(func1.calling_convention == CallingConvention::Internal);
        let func2 = compile_func_decl("@[inline(always)] fn foo() {}");
        assert!(func2.inline == Inline::Always);
        assert!(func2.calling_convention == CallingConvention::Internal);
        let func3 = compile_func_decl("@[inline(always), cc(abi)] fn foo() {}");
        assert!(func3.inline == Inline::Always);
        assert!(func3.calling_convention == CallingConvention::Abi);
        let func4 = compile_func_decl("@[inline(never), cc(internal)] fn foo() {}");
        assert!(func4.inline == Inline::Never);
        assert!(func4.calling_convention == CallingConvention::Internal);

        let vd1 = compile_vardecl("@[] let x: int = 5;");
        assert!(!vd1.is_extern);
        let vd2 = compile_vardecl("@[extern] let x: int;");
        assert!(vd2.is_extern);

        compile_structdecl("@[] struct Test {}");
    }

    #[test]
    fn test_global_decl_invalid() {
        use utils::*;
        let fails = ["fn f()", "fn f();", "let a: int = 5"];
        for s in fails {
            let mut ctx = ctx();
            let lexed = lex(s, &mut ctx);
            let mut parser = Parser::new(&lexed);
            assert!(
                parser.parse_global_decl(&mut ctx).is_err()
                    || !parser.is_at_end()
                    || !parser.errors.is_empty()
            );
        }
    }

    #[track_caller]
    fn compare_program(s: &str, expected: Program) {
        use utils::*;
        let mut ctx = ctx();
        let lexed = lex(s, &mut ctx);
        let parsed = Parser::parse_test(&lexed, &mut ctx);
        assert!(!parsed.has_errors());
        assert!(parsed.program.ctx_eq(&expected, &ctx));
    }

    #[test]
    fn test_program_valid() {
        use utils::*;
        let func = func_decl(ident("func"), vec![], None, bblock(vec![]));
        let struct_ = struct_decl(ident("MyStruct"), vec![(ident("a"), get_type("int"))]);
        let var_decl = variable_decl(
            ident("b"),
            get_type("*int"),
            Some(cast(num(5), get_type("*int"))),
        );
        let StmtKind::VariableDeclaration(var_decl) = var_decl.kind else {
            unreachable!();
        };
        let v = vec![
            GlobalDeclaration {
                kind: GlobalDeclarationKind::Function(func),
                span: Span::empty(),
                id: NodeId(0),
            },
            GlobalDeclaration {
                kind: GlobalDeclarationKind::Struct(struct_),
                span: Span::empty(),
                id: NodeId(0),
            },
            GlobalDeclaration {
                kind: GlobalDeclarationKind::Variable(var_decl),
                span: Span::empty(),
                id: NodeId(0),
            },
        ];
        let program = Program { decls: v };
        compare_program(
            "fn func() {} struct MyStruct {a: int} let b: *int = 5 as *int;",
            program,
        );
    }

    #[test]
    fn test_program_invalid() {
        use utils::*;
        let fails = [
            "fn f() {}; let a;",
            "let a: int = 5 fn f() {}",
            "let a: int = 5",
        ];
        for s in fails {
            let mut ctx = ctx();
            let lexed = lex(s, &mut ctx);
            let parsed = Parser::parse_test(&lexed, &mut ctx);
            assert!(parsed.has_errors());
        }
    }
}
