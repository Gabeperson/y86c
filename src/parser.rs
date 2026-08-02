use bumpalo::Bump;
use bumpalo::collections::Vec as BumpVec;

use crate::ast::*;
use crate::lexer::{KeywordKind, Token, TokenKind};
use crate::span::Span;

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
    IntLiteralTooBig {
        found: Token,
    },
    UnexpectedEndOfInput,
}

#[derive(Debug)]
pub struct Parser<'t> {
    tokens: &'t Vec<Token>,
    errors: Vec<ParsingError>,
    cursor: usize,
}

#[derive(Debug)]
pub struct ParseOutput<'a> {
    pub program: Program<'a>,
    pub errors: Vec<ParsingError>,
}

impl ParseOutput<'_> {
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

impl<'t> Parser<'t> {
    pub fn new(tokens: &'t Vec<Token>) -> Self {
        Self {
            tokens,
            errors: Vec::new(),
            cursor: 0,
        }
    }
    pub fn parse<'a>(mut self, bump: &'a Bump) -> ParseOutput<'a> {
        let mut decls = BumpVec::new_in(bump);
        while !self.is_at_end() {
            let decl = self.parse_global_decl(bump);
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
                    | TokenKind::Keyword(KeywordKind::Fn)
                    | TokenKind::Keyword(KeywordKind::Let) = next.kind
                    {
                        return Ok(());
                    }
                }
                TokenKind::Semicolon => {
                    let next = self.next()?;
                    if let TokenKind::Keyword(KeywordKind::Struct)
                    | TokenKind::Keyword(KeywordKind::Fn) = next.kind
                    {
                        return Ok(());
                    }
                }
                _ => {}
            }
            self.advance()
        }
    }
    fn parse_struct_init<'a>(&mut self, bump: &'a Bump) -> Result<Expr<'a>> {
        let ident = self.parse_ident(bump)?;
        self.expect(
            TokenKind::LCurly,
            "Expected '{' after struct name in struct initialization",
        )?;
        let mut field_inits = BumpVec::new_in(bump);
        while !self.match_token(TokenKind::RCurly)? {
            let name = self.parse_ident(bump)?;
            self.expect(
                TokenKind::Colon,
                "Expected ':' after member name in struct initialization",
            )?;
            let value = self.parse_expr(bump)?;
            field_inits.push((name, value));
            if self.match_token(TokenKind::Comma)? {
                self.advance();
            } else {
                break;
            }
        }
        let close_curly = self.expect(
            TokenKind::RCurly,
            "Expected ')' at end of struct initialization",
        )?;
        let span = Span::new(ident.span.start, close_curly.span.end);
        let init = bump.alloc(ExprKind::StructInit(StructInit {
            name: ident,
            field_inits,
            span,
        }));
        Ok(Expr::new(init, span))
    }
    fn parse_array_init<'a>(&mut self, bump: &'a Bump) -> Result<Expr<'a>> {
        let token = self.expect_or_ice(TokenKind::LSquare);
        let mut elements = BumpVec::new_in(bump);
        while !self.match_token(TokenKind::RSquare)? {
            let elem = self.parse_expr(bump)?;
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
        let init = bump.alloc(ExprKind::ArrayInit(ArrayInit { elements, span }));
        Ok(Expr::new(init, span))
    }
    fn parse_global_decl<'a>(&mut self, bump: &'a Bump) -> Result<GlobalDeclaration<'a>> {
        let token = self.current()?;
        match token.kind {
            TokenKind::Keyword(KeywordKind::Struct) => {
                let decl = self.parse_struct_decl(bump)?;
                let span = decl.span;
                Ok(GlobalDeclaration {
                    kind: GlobalDeclarationKind::Struct(decl),
                    span,
                })
            }
            TokenKind::Keyword(KeywordKind::Fn) => {
                let decl = self.parse_function_decl(bump)?;
                let span = decl.span;
                Ok(GlobalDeclaration {
                    kind: GlobalDeclarationKind::Function(decl),
                    span,
                })
            }
            TokenKind::Keyword(KeywordKind::Let) => {
                let decl = self.parse_variable_decl(bump)?;
                if !self.match_token(TokenKind::Semicolon)? {
                    self.errors.push(ParsingError::ExpectedOtherToken {
                        expected: TokenKind::Semicolon,
                        found: self.current()?,
                        msg: "Expected ';' after global-level variable declaration",
                    });
                }
                let span = decl.span;
                Ok(GlobalDeclaration {
                    kind: GlobalDeclarationKind::Variable(decl),
                    span,
                })
            }
            _ => Err(ParsingError::ExpectedGlobalDecl { found: token }),
        }
    }
    fn parse_stmt<'a>(&mut self, bump: &'a Bump) -> Result<Stmt<'a>> {
        let token = self.current()?;
        let stmt = match token.kind {
            TokenKind::LCurly => {
                let block = self.parse_block(bump)?;
                let span = block.span;
                let kind = bump.alloc(StmtKind::Block(block));
                Stmt { kind, span }
            }
            TokenKind::Keyword(KeywordKind::Continue) => {
                self.advance();
                let span = token.span;
                let kind = bump.alloc(StmtKind::Continue(Continue { span }));
                Stmt { kind, span }
            }
            TokenKind::Keyword(KeywordKind::Break) => {
                self.advance();
                let span = token.span;
                let kind = bump.alloc(StmtKind::Break(Break { span }));
                Stmt { kind, span }
            }
            TokenKind::Keyword(KeywordKind::Return) => self.parse_return_stmt(bump)?,
            TokenKind::Keyword(KeywordKind::While) => {
                return self.parse_while_loop(bump);
            }
            TokenKind::Keyword(KeywordKind::For) => {
                return self.parse_for_loop(bump);
            }
            TokenKind::Keyword(KeywordKind::If) => {
                return self.parse_if_stmt(bump);
            }
            TokenKind::Keyword(KeywordKind::Let) => {
                let decl = self.parse_variable_decl(bump)?;
                let span = token.span;
                let kind = bump.alloc(StmtKind::VariableDeclaration(decl));
                Stmt { kind, span }
            }
            #[cfg(test)]
            TokenKind::Keyword(KeywordKind::Assert) => self.parse_assert(bump)?,
            _ => {
                let expr = self.parse_expr(bump)?;
                let span = expr.span;
                let kind = bump.alloc(StmtKind::Expr(expr));
                Stmt { kind, span }
            }
        };
        if self.match_token(TokenKind::Semicolon)? {
            self.advance()
        } else {
            self.recover_stmt()?;
        }
        Ok(stmt)
    }
    fn parse_expr<'a>(&mut self, bump: &'a Bump) -> Result<Expr<'a>> {
        self.expr_bp(0, bump)
    }
    fn recover_stmt(&mut self) -> Result<()> {
        loop {
            let token = self.current()?;
            match token.kind {
                TokenKind::Semicolon => {
                    self.advance();
                    return Ok(());
                }
                TokenKind::RCurly => return Ok(()),
                _ => self.advance(),
            }
        }
    }
    fn parse_type<'a>(&mut self, bump: &'a Bump) -> Result<Type<'a>> {
        while self.current()?.kind == TokenKind::Error {
            self.advance();
        }
        let token = self.current()?;
        match token.kind {
            TokenKind::Keyword(KeywordKind::Int) => {
                self.advance();
                let kind = bump.alloc(TypeKind::Int(IntType::I64));
                Ok(Type::new(kind, token.span))
            }
            TokenKind::Keyword(KeywordKind::Fn) => {
                self.advance();
                self.expect(
                    TokenKind::LParen,
                    "Expected '(' after 'fn' in function pointer",
                )?;
                let mut param_types = BumpVec::new_in(bump);
                while !self.match_token(TokenKind::RParen)? {
                    param_types.push(self.parse_type(bump)?);
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
                    let typ = self.parse_type(bump)?;
                    span.end = typ.span.end;
                    Some(typ)
                } else {
                    None
                };
                let kind = bump.alloc(TypeKind::FuncPtr {
                    return_type,
                    param_types,
                });

                Ok(Type::new(kind, span))
            }
            TokenKind::Keyword(KeywordKind::Void) => {
                self.advance();
                let kind = bump.alloc(TypeKind::Void);
                Ok(Type::new(kind, token.span))
            }
            TokenKind::Asterisk => {
                let mut count = 0;
                while self.match_token(TokenKind::Asterisk)? {
                    count += 1;
                    self.advance();
                }
                let mut typ = self.parse_type(bump)?;
                for i in 0..count {
                    let end = typ.span.end;
                    typ = Type::new(
                        bump.alloc(TypeKind::Ptr { pointee: typ }),
                        Span::new(token.span.start + i, end),
                    );
                }
                Ok(typ)
            }
            TokenKind::Ident(_) => {
                let ident = self.parse_ident(bump)?;
                let span = ident.span;
                let kind = bump.alloc(TypeKind::Struct { name: ident });
                Ok(Type::new(kind, span))
            }
            TokenKind::LSquare => {
                self.advance();
                let element_type = self.parse_type(bump)?;
                self.expect(
                    TokenKind::Semicolon,
                    "Expected ';' in array type separating element type and size",
                )?;
                let token = self.current()?;
                let TokenKind::IntLiteral { value } = token.kind else {
                    return Err(ParsingError::ExpectedArraySize { found: token });
                };
                self.advance();
                let end_tok =
                    self.expect(TokenKind::RSquare, "Expected ']' at end of array type")?;
                let kind = bump.alloc(TypeKind::Array {
                    element_type,
                    size: value,
                });

                Ok(Type::new(
                    kind,
                    Span::new(token.span.start, end_tok.span.end),
                ))
            }
            TokenKind::Comma | TokenKind::Equal | TokenKind::Semicolon | TokenKind::RParen => {
                self.errors
                    .push(ParsingError::ExpectedType { found: token });
                let alloced = bump.alloc(TypeKind::Error);
                Ok(Type::new(alloced, Span::empty()))
            }
            _ => Err(ParsingError::ExpectedType { found: token }),
        }
    }
    #[cfg(test)]
    fn parse_assert<'a>(&mut self, bump: &'a Bump) -> Result<Stmt<'a>> {
        let token = self.current()?;
        self.advance();
        self.expect(TokenKind::LParen, "Expected '(' after assert")?;
        let expr = self.parse_expr(bump)?;
        let rparen = self.expect(TokenKind::RParen, "Expected ')' after assert expression")?;
        let span = Span::new(token.span.start, rparen.span.end);
        let kind = bump.alloc(StmtKind::Assert(Assert {
            condition: expr,
            span,
        }));
        Ok(Stmt { kind, span })
    }
    fn parse_ident_type_pair<'a>(&mut self, bump: &'a Bump) -> Result<(Ident<'a>, Type<'a>)> {
        let ident = self.parse_ident(bump)?;
        self.expect(
            TokenKind::Colon,
            "Expected ':' after ident in ident: type pair",
        )?;
        let typ = self.parse_type(bump)?;
        Ok((ident, typ))
    }
    fn parse_variable_decl<'a>(&mut self, bump: &'a Bump) -> Result<VariableDeclaration<'a>> {
        let let_kw = self.expect_or_ice(TokenKind::Keyword(KeywordKind::Let));
        let name = self.parse_ident(bump)?;
        self.expect(
            TokenKind::Colon,
            "Expected ':' after variable name in variable declaration",
        )?;
        let typ = self.parse_type(bump)?;
        let mut span = Span::new(let_kw.span.start, typ.span.end);
        let init_value = if self.match_token(TokenKind::Equal)? {
            self.advance();
            let expr = self.parse_expr(bump)?;
            span.end = expr.span.end;
            Some(self.parse_expr(bump)?)
        } else {
            None
        };
        Ok(VariableDeclaration {
            var_type: typ,
            name,
            init_value,
            span,
        })
    }
    fn parse_function_decl<'a>(&mut self, bump: &'a Bump) -> Result<FunctionDeclaration<'a>> {
        let fn_kw = self.expect_or_ice(TokenKind::Keyword(KeywordKind::Fn));
        let name = self.parse_ident(bump)?;
        self.expect(
            TokenKind::LParen,
            "Expected '(' after function name in declaration",
        )?;
        let mut params = BumpVec::new_in(bump);
        while !self.match_token(TokenKind::RParen)? {
            let pair = self.parse_ident_type_pair(bump)?;
            params.push(pair);
            if self.match_token(TokenKind::Comma)? {
                self.advance();
            } else {
                break;
            }
        }
        self.expect(TokenKind::RParen, "Expected ')' after function parameters")?;
        let token = self.current()?;
        let ret_type = match token.kind {
            TokenKind::LCurly => None,
            TokenKind::Arrow => {
                self.advance();
                Some(self.parse_type(bump)?)
            }
            _ => {
                return Err(ParsingError::ExpectedReturnType { found: token });
            }
        };
        let body = self.parse_block(bump)?;
        let span = Span::new(fn_kw.span.start, body.span.end);
        Ok(FunctionDeclaration {
            return_type: ret_type,
            name,
            params,
            body,
            span,
        })
    }
    fn parse_struct_decl<'a>(&mut self, bump: &'a Bump) -> Result<StructDeclaration<'a>> {
        let struct_kw = self.expect_or_ice(TokenKind::Keyword(KeywordKind::Struct));
        let name = self.parse_ident(bump)?;
        self.expect(TokenKind::LCurly, "Expected '{' after struct name")?;
        let mut members = BumpVec::new_in(bump);
        while !self.match_token(TokenKind::RCurly)? {
            let pair = self.parse_ident_type_pair(bump)?;
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
        Ok(StructDeclaration {
            name,
            fields: members,
            span,
        })
    }
    fn parse_block<'a>(&mut self, bump: &'a Bump) -> Result<Block<'a>> {
        let lcurly = self.expect_or_ice(TokenKind::LCurly);
        let mut statements = BumpVec::new_in(bump);
        while !self.match_token(TokenKind::RCurly)? {
            match self.parse_stmt(bump) {
                Ok(s) => statements.push(s),
                Err(e) => {
                    self.recover_stmt()?;
                    self.errors.push(e);
                }
            }
        }
        let end = self.expect(TokenKind::RCurly, "Expected '}' at end of block")?;
        let span = Span::new(lcurly.span.start, end.span.end);
        Ok(Block {
            body: statements,
            span,
        })
    }
    fn parse_if_stmt<'a>(&mut self, bump: &'a Bump) -> Result<Stmt<'a>> {
        let if_kw = self.expect_or_ice(TokenKind::Keyword(KeywordKind::If));
        self.expect(TokenKind::LParen, "Expected '(' after 'if'")?;
        let condition = self.parse_expr(bump)?;
        self.expect(TokenKind::RParen, "Expected ')' after if condition")?;
        let then_branch = self.parse_stmt(bump)?;
        let mut span = Span::new(if_kw.span.start, then_branch.span.end);
        let else_branch = if self.match_token(TokenKind::Keyword(KeywordKind::Else))? {
            self.advance();
            let stmt = self.parse_stmt(bump)?;
            span.end = stmt.span.end;
            Some(stmt)
        } else {
            None
        };
        let kind = bump.alloc(StmtKind::IfStmt(IfStmt {
            condition,
            then_branch,
            else_branch,
            span,
        }));
        Ok(Stmt { kind, span })
    }
    fn parse_while_loop<'a>(&mut self, bump: &'a Bump) -> Result<Stmt<'a>> {
        let while_kw = self.expect_or_ice(TokenKind::Keyword(KeywordKind::While));
        self.expect(TokenKind::LParen, "Expected '(' after 'while'")?;
        let condition = self.parse_expr(bump)?;
        self.expect(TokenKind::RParen, "Expected ')' after while loop condition")?;
        let body = self.parse_stmt(bump)?;
        let span = Span::new(while_kw.span.start, body.span.end);
        let kind = bump.alloc(StmtKind::WhileLoop(WhileLoop {
            condition,
            body,
            span,
        }));
        Ok(Stmt { kind, span })
    }
    fn parse_for_loop<'a>(&mut self, bump: &'a Bump) -> Result<Stmt<'a>> {
        let for_kw = self.expect_or_ice(TokenKind::Keyword(KeywordKind::For));
        self.expect(TokenKind::LParen, "Expected '(' after 'for'")?;
        let token = self.current()?;
        let init = match token.kind {
            TokenKind::Keyword(KeywordKind::Let) => {
                let vardecl = self.parse_variable_decl(bump)?;
                let span = vardecl.span;
                let kind = bump.alloc(StmtKind::VariableDeclaration(vardecl));
                Some(Stmt { kind, span })
            }
            TokenKind::Semicolon => None,
            _ => {
                let expr = self.parse_expr(bump)?;
                let span = expr.span;
                let kind = bump.alloc(StmtKind::Expr(expr));
                Some(Stmt { kind, span })
            }
        };
        self.expect(TokenKind::Semicolon, "Expected ';' after loop initializer")?;
        let condition = match self.match_token(TokenKind::Semicolon)? {
            false => Some(self.parse_expr(bump)?),
            true => None,
        };
        self.expect(TokenKind::Semicolon, "Expected ';' after loop condition")?;
        let post = match self.match_token(TokenKind::RParen)? {
            false => Some(self.parse_expr(bump)?),
            true => None,
        };
        self.expect(TokenKind::RParen, "Expected ')' after for loop increment")?;
        let body = self.parse_stmt(bump)?;
        let span = Span::new(for_kw.span.start, body.span.end);
        let kind = bump.alloc(StmtKind::ForLoop(ForLoop {
            init,
            condition,
            post,
            body,
            span,
        }));
        Ok(Stmt { kind, span })
    }
    fn parse_return_stmt<'a>(&mut self, bump: &'a Bump) -> Result<Stmt<'a>> {
        let return_kw = self.expect_or_ice(TokenKind::Keyword(KeywordKind::Return));
        if self.match_token(TokenKind::Semicolon)? {
            let stmt = bump.alloc(StmtKind::ReturnStmt(ReturnStmt {
                value: None,
                span: return_kw.span,
            }));
            return Ok(Stmt {
                kind: stmt,
                span: return_kw.span,
            });
        }
        let value = self.parse_expr(bump)?;
        let span = Span::new(return_kw.span.start, value.span.end);

        let stmt = bump.alloc(StmtKind::ReturnStmt(ReturnStmt {
            value: Some(value),
            span,
        }));
        Ok(Stmt { kind: stmt, span })
    }
    fn parse_ident<'a>(&mut self, bump: &'a Bump) -> Result<Ident<'a>> {
        let token = self.current()?;
        let TokenKind::Ident(id) = token.kind else {
            return Err(ParsingError::ExpectedIdent { found: token });
        };
        self.advance();
        let s = bump.alloc_str(&id);
        Ok(Ident {
            ident: s,
            span: token.span,
        })
    }
}

impl<'t> Parser<'t> {
    fn expr_bp<'a>(&mut self, min_bp: u8, bump: &'a Bump) -> Result<Expr<'a>> {
        let token = self.current()?;
        let mut lhs = if let Some(((), bp)) = prefix_binding_power(&token.kind) {
            self.advance();
            let expr = self.expr_bp(bp, bump)?;
            self.handle_prefix(token, expr, bump)?
        } else {
            let Some(atom) = self.parse_atom(bump)? else {
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
                lhs = self.handle_postfix(lhs, token, bump)?;
                continue;
            }
            if let Some((l_bp, r_bp)) = infix_binding_power(&token.kind) {
                if l_bp < min_bp {
                    break;
                }
                self.advance();
                if let TokenKind::QuestionMark = token.kind {
                    let true_branch = self.parse_expr(bump)?;
                    self.expect(TokenKind::Colon, "Expected ':' in ternary operator")?;
                    let false_branch = self.expr_bp(r_bp, bump)?;
                    let condition = lhs;
                    let span = Span::new(condition.span.start, false_branch.span.end);
                    let kind = bump.alloc(ExprKind::Ternary(Ternary {
                        condition,
                        true_branch,
                        false_branch,
                        span,
                    }));
                    lhs = Expr::new(kind, span);
                    continue;
                }
                let rhs = self.expr_bp(r_bp, bump)?;
                lhs = self.handle_infix(lhs, token, rhs, bump)?;
                continue;
            }
            break;
        }
        Ok(lhs)
    }
    fn parse_atom<'a>(&mut self, bump: &'a Bump) -> Result<Option<Expr<'a>>> {
        let token = self.current()?;
        match token.kind {
            TokenKind::Keyword(KeywordKind::Sizeof) => {
                self.advance();
                self.expect(TokenKind::LParen, "Expected '(' after sizeof")?;
                let typ = self.parse_type(bump)?;
                let end_tok = self.expect(TokenKind::RParen, "Expected ')' after sizeof type")?;
                let span = Span::new(token.span.start, end_tok.span.end);
                let kind = bump.alloc(ExprKind::SizeOfType(SizeOfType { typ, span }));
                Ok(Some(Expr::new(kind, span)))
            }
            TokenKind::Keyword(KeywordKind::Nullptr) => {
                self.advance();
                let kind = bump.alloc(ExprKind::Nullptr(Nullptr));
                Ok(Some(Expr::new(kind, token.span)))
            }
            TokenKind::LParen => {
                self.advance();
                let expr = self.parse_expr(bump)?;
                self.expect(TokenKind::RParen, "Expected ')' after parenthesized expr")?;
                Ok(Some(expr))
            }
            TokenKind::LSquare => Ok(Some(self.parse_array_init(bump)?)),
            TokenKind::IntLiteral { value } => {
                self.advance();
                let span = token.span;
                let value: i64 = match value.try_into() {
                    Ok(v) => v,
                    Err(_e) => {
                        self.errors
                            .push(ParsingError::IntLiteralTooBig { found: token });
                        0
                    }
                };
                let kind = bump.alloc(ExprKind::Int(Int {
                    lit: IntLiteral {
                        kind: IntLiteralKind::I64(value),
                        span,
                    },
                    span,
                }));
                Ok(Some(Expr::new(kind, span)))
            }
            TokenKind::Ident(s) => {
                if let Ok(Token {
                    kind: TokenKind::LCurly,
                    ..
                }) = self.next()
                {
                    Ok(Some(self.parse_struct_init(bump)?))
                } else {
                    self.advance();
                    let s = bump.alloc_str(&s);
                    let kind = bump.alloc(ExprKind::Ident(Ident {
                        ident: s,
                        span: token.span,
                    }));
                    Ok(Some(Expr::new(kind, token.span)))
                }
            }
            _ => Ok(None),
        }
    }

    fn handle_prefix<'a>(&mut self, op: Token, expr: Expr<'a>, bump: &'a Bump) -> Result<Expr<'a>> {
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
        let kind = bump.alloc(ExprKind::PrefixOp(PrefixOp { kind, expr, span }));
        Ok(Expr::new(kind, span))
    }
    fn handle_infix<'a>(
        &mut self,
        lhs: Expr<'a>,
        op: Token,
        rhs: Expr<'a>,
        bump: &'a Bump,
    ) -> Result<Expr<'a>> {
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
            _ => unreachable!(),
        };
        let span = Span::new(lhs.span.start, rhs.span.end);
        let exprkind = bump.alloc(ExprKind::BinaryOp(BinaryOp {
            kind: binop_kind,
            left: lhs,
            right: rhs,
            span,
        }));
        Ok(Expr::new(exprkind, span))
    }
    fn handle_postfix<'a>(
        &mut self,
        expr: Expr<'a>,
        op: Token,
        bump: &'a Bump,
    ) -> Result<Expr<'a>> {
        self.advance();
        match op.kind {
            TokenKind::LParen => {
                let mut args = BumpVec::new_in(bump);
                while !self.match_token(TokenKind::RParen)? {
                    args.push(self.parse_expr(bump)?);
                    if self.match_token(TokenKind::Comma)? {
                        self.advance();
                    } else {
                        break;
                    }
                }
                let rparen = self.expect(TokenKind::RParen, "Expected ')' after argument list")?;
                let span = Span::new(expr.span.start, rparen.span.end);
                let kind = bump.alloc(ExprKind::FunctionCall(FunctionCall {
                    func_expr: expr,
                    args,
                    span,
                }));
                Ok(Expr::new(kind, span))
            }
            TokenKind::LSquare => {
                let index = self.parse_expr(bump)?;
                let rsquare = self.expect(
                    TokenKind::RSquare,
                    "Expected ']' after array index expression",
                )?;
                let span = Span::new(expr.span.start, rsquare.span.end);
                let kind = bump.alloc(ExprKind::ArrayIndex(ArrayIndex {
                    array: expr,
                    index,
                    span,
                }));
                Ok(Expr::new(kind, span))
            }
            TokenKind::Period => {
                let ident = self.parse_ident(bump)?;
                let span = Span::new(expr.span.start, ident.span.end);
                let kind = bump.alloc(ExprKind::MemberAccess(MemberAccess {
                    struct_expr: expr,
                    member_name: ident,
                    span,
                }));
                Ok(Expr::new(kind, span))
            }
            TokenKind::Arrow => {
                let ident = self.parse_ident(bump)?;
                let span = Span::new(expr.span.start, ident.span.end);
                let kind = bump.alloc(ExprKind::PointerMemberAccess(PointerMemberAccess {
                    struct_ptr_expr: expr,
                    member_name: ident,
                    span,
                }));
                Ok(Expr::new(kind, span))
            }
            TokenKind::DoublePlus | TokenKind::DoubleMinus => {
                let span = Span::new(expr.span.start, op.span.end);
                let kind = bump.alloc(ExprKind::PostfixOp(PostfixOp {
                    kind: if op.kind == TokenKind::DoublePlus {
                        PostfixOpKind::Increment
                    } else {
                        PostfixOpKind::Decrement
                    },
                    expr,
                    span,
                }));
                Ok(Expr::new(kind, span))
            }
            TokenKind::Keyword(KeywordKind::As) => {
                let typ = self.parse_type(bump)?;
                let span = Span::new(expr.span.start, typ.span.end);
                let kind = bump.alloc(ExprKind::Cast(Cast { to_type: typ, expr }));
                Ok(Expr::new(kind, span))
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
        | TokenKind::Equal => (10, 9),
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
    use crate::lexer::Lexer;

    use super::*;
    fn compare_types(s: &str, expected: Type<'_>) {
        use utils::*;
        let tokens = lex(s);
        let mut parser = Parser::new(&tokens);
        let bump = Bump::new();
        let parsed = parser.parse_type(&bump);
        let typ = match parsed {
            Ok(t) => t,
            Err(_e) => {
                panic!("{s} did not parse properly");
            }
        };
        assert!(parser.is_at_end());
        assert_eq!(typ, expected);
    }
    #[macro_use]
    mod utils {
        use bumpalo::collections::CollectIn;

        use super::*;
        pub fn lex(s: &str) -> Vec<Token> {
            let lexer = Lexer::new(s);
            let lexed = lexer.lex();
            assert!(!lexed.has_errors());
            lexed.tokens
        }
        pub fn a<'a>(bump: &'a Bump) -> Expr<'a> {
            Expr::new(bump.alloc(ExprKind::Ident(ident("a"))), Span::empty())
        }
        pub fn b<'a>(bump: &'a Bump) -> Expr<'a> {
            Expr::new(bump.alloc(ExprKind::Ident(ident("b"))), Span::empty())
        }
        pub fn c<'a>(bump: &'a Bump) -> Expr<'a> {
            Expr::new(bump.alloc(ExprKind::Ident(ident("c"))), Span::empty())
        }
        pub fn d<'a>(bump: &'a Bump) -> Expr<'a> {
            Expr::new(bump.alloc(ExprKind::Ident(ident("d"))), Span::empty())
        }
        pub fn e<'a>(bump: &'a Bump) -> Expr<'a> {
            Expr::new(bump.alloc(ExprKind::Ident(ident("e"))), Span::empty())
        }
        pub fn f<'a>(bump: &'a Bump) -> Expr<'a> {
            Expr::new(bump.alloc(ExprKind::Ident(ident("f"))), Span::empty())
        }
        pub fn g<'a>(bump: &'a Bump) -> Expr<'a> {
            Expr::new(bump.alloc(ExprKind::Ident(ident("g"))), Span::empty())
        }
        pub fn ident<'a>(s: &'a str) -> Ident<'a> {
            Ident {
                ident: s,
                span: Span::empty(),
            }
        }
        pub fn ptr<'a>(pointee: Type<'a>, bump: &'a Bump) -> Type<'a> {
            Type::new(bump.alloc(TypeKind::Ptr { pointee }), Span::empty())
        }
        pub fn void() -> Type<'static> {
            Type::new(&TypeKind::Void, Span::empty())
        }
        pub fn int() -> Type<'static> {
            Type::new(&TypeKind::Int(IntType::I64), Span::empty())
        }
        pub fn struct_<'a>(s: &'a str, bump: &'a Bump) -> Type<'a> {
            Type::new(
                bump.alloc(TypeKind::Struct { name: ident(s) }),
                Span::empty(),
            )
        }
        pub fn array<'a>(t: Type<'a>, len: u64, bump: &'a Bump) -> Type<'a> {
            Type::new(
                bump.alloc(TypeKind::Array {
                    element_type: t,
                    size: len,
                }),
                Span::empty(),
            )
        }
        pub fn func_ptr<'a>(
            param_types: Vec<Type<'a>>,
            return_type: Option<Type<'a>>,
            bump: &'a Bump,
        ) -> Type<'a> {
            let mut v = BumpVec::new_in(bump);
            for i in param_types {
                v.push(i.clone())
            }
            Type::new(
                bump.alloc(TypeKind::FuncPtr {
                    return_type,
                    param_types: v,
                }),
                Span::empty(),
            )
        }
        pub fn cast<'a>(from: Expr<'a>, to: Type<'a>, bump: &'a Bump) -> Expr<'a> {
            Expr::new(
                bump.alloc(ExprKind::Cast(Cast {
                    to_type: to,
                    expr: from,
                })),
                Span::empty(),
            )
        }
        pub fn binop<'a>(
            lhs: Expr<'a>,
            rhs: Expr<'a>,
            op_type: BinaryOpKind,
            bump: &'a Bump,
        ) -> Expr<'a> {
            Expr::new(
                bump.alloc(ExprKind::BinaryOp(BinaryOp {
                    kind: op_type,
                    left: lhs,
                    right: rhs,
                    span: Span::empty(),
                })),
                Span::empty(),
            )
        }
        pub fn prefix_op<'a>(expr: Expr<'a>, op_type: PrefixOpKind, bump: &'a Bump) -> Expr<'a> {
            Expr::new(
                bump.alloc(ExprKind::PrefixOp(PrefixOp {
                    kind: op_type,
                    expr,
                    span: Span::empty(),
                })),
                Span::empty(),
            )
        }
        pub fn postfix_op<'a>(expr: Expr<'a>, op_type: PostfixOpKind, bump: &'a Bump) -> Expr<'a> {
            Expr::new(
                bump.alloc(ExprKind::PostfixOp(PostfixOp {
                    kind: op_type,
                    expr,
                    span: Span::empty(),
                })),
                Span::empty(),
            )
        }
        pub fn ternary<'a>(
            condition: Expr<'a>,
            true_branch: Expr<'a>,
            false_branch: Expr<'a>,
            bump: &'a Bump,
        ) -> Expr<'a> {
            Expr::new(
                bump.alloc(ExprKind::Ternary(Ternary {
                    condition,
                    true_branch,
                    false_branch,
                    span: Span::empty(),
                })),
                Span::empty(),
            )
        }
        pub fn func_call<'a>(func_expr: Expr<'a>, args: Vec<Expr<'a>>, bump: &'a Bump) -> Expr<'a> {
            Expr::new(
                bump.alloc(ExprKind::FunctionCall(FunctionCall {
                    func_expr,
                    args: args.into_iter().collect_in(bump),
                    span: Span::empty(),
                })),
                Span::empty(),
            )
        }
        pub fn member_access<'a, 's: 'a>(
            expr: Expr<'a>,
            member: &'s str,
            bump: &'a Bump,
        ) -> Expr<'a> {
            Expr::new(
                bump.alloc(ExprKind::MemberAccess(MemberAccess {
                    struct_expr: expr,
                    member_name: ident(member),
                    span: Span::empty(),
                })),
                Span::empty(),
            )
        }
        pub fn pointer_member_access<'a, 's: 'a>(
            expr: Expr<'a>,
            member: &'s str,
            bump: &'a Bump,
        ) -> Expr<'a> {
            Expr::new(
                bump.alloc(ExprKind::PointerMemberAccess(PointerMemberAccess {
                    struct_ptr_expr: expr,
                    member_name: ident(member),
                    span: Span::empty(),
                })),
                Span::empty(),
            )
        }
        pub fn array_index<'a>(expr: Expr<'a>, index: Expr<'a>, bump: &'a Bump) -> Expr<'a> {
            Expr::new(
                bump.alloc(ExprKind::ArrayIndex(ArrayIndex {
                    array: expr,
                    index,
                    span: Span::empty(),
                })),
                Span::empty(),
            )
        }
        pub fn sizeof<'a>(typ: Type<'a>, bump: &'a Bump) -> Expr<'a> {
            Expr::new(
                bump.alloc(ExprKind::SizeOfType(SizeOfType {
                    typ,
                    span: Span::empty(),
                })),
                Span::empty(),
            )
        }
        pub fn nullptr<'a>(bump: &'a Bump) -> Expr<'a> {
            Expr::new(bump.alloc(ExprKind::Nullptr(Nullptr)), Span::empty())
        }
        pub fn get_type<'a>(s: &str, bump: &'a Bump) -> Type<'a> {
            let tokens = lex(s);
            let mut parser = Parser::new(&tokens);
            let output = parser.parse_type(bump).unwrap();
            assert!(parser.is_at_end(), "{parser:?}");
            output
        }

        macro_rules! anchor {
            ($bump:ident) => {{
                let ptr = |pointee| ptr(pointee, $bump);
                let struct_ = |s| struct_(s, $bump);
                let array = |t, len| array(t, len, $bump);
                let func_ptr = |param_types, return_type| func_ptr(param_types, return_type, $bump);
                let cast = |from, to| cast(from, to, $bump);

                let ternary = |cond, t_br, f_br| ternary(cond, t_br, f_br, $bump);
                let func_call = |func_expr, args| func_call(func_expr, args, $bump);
                let member_access = |expr, member| member_access(expr, member, $bump);
                let pointer_member_access =
                    |expr, member| pointer_member_access(expr, member, $bump);
                let array_index = |expr, index| array_index(expr, index, $bump);
                let sizeof = |typ| sizeof(typ, $bump);
                let nullptr = || nullptr($bump);
                let get_type = |s| get_type(s, $bump);
                let add = |lhs, rhs| binop(lhs, rhs, BinaryOpKind::Add, $bump);
                let sub = |lhs, rhs| binop(lhs, rhs, BinaryOpKind::Sub, $bump);
                let mul = |lhs, rhs| binop(lhs, rhs, BinaryOpKind::Mul, $bump);
                let div = |lhs, rhs| binop(lhs, rhs, BinaryOpKind::Div, $bump);
                let eq = |lhs, rhs| binop(lhs, rhs, BinaryOpKind::Eq, $bump);
                let gt = |lhs, rhs| binop(lhs, rhs, BinaryOpKind::Greater, $bump);
                let lt = |lhs, rhs| binop(lhs, rhs, BinaryOpKind::Less, $bump);
                let geq = |lhs, rhs| binop(lhs, rhs, BinaryOpKind::GreaterOrEqual, $bump);
                let leq = |lhs, rhs| binop(lhs, rhs, BinaryOpKind::LessOrEqual, $bump);
                let noteq = |lhs, rhs| binop(lhs, rhs, BinaryOpKind::NotEq, $bump);
                let and = |lhs, rhs| binop(lhs, rhs, BinaryOpKind::And, $bump);
                let or = |lhs, rhs| binop(lhs, rhs, BinaryOpKind::Or, $bump);
                let bitand = |lhs, rhs| binop(lhs, rhs, BinaryOpKind::BitAnd, $bump);
                let bitor = |lhs, rhs| binop(lhs, rhs, BinaryOpKind::BitOr, $bump);
                let xor = |lhs, rhs| binop(lhs, rhs, BinaryOpKind::Xor, $bump);
                let mod_ = |lhs, rhs| binop(lhs, rhs, BinaryOpKind::Mod, $bump);
                let addeq = |lhs, rhs| binop(lhs, rhs, BinaryOpKind::AddAssign, $bump);
                let subeq = |lhs, rhs| binop(lhs, rhs, BinaryOpKind::SubAssign, $bump);
                let muleq = |lhs, rhs| binop(lhs, rhs, BinaryOpKind::MulAssign, $bump);
                let diveq = |lhs, rhs| binop(lhs, rhs, BinaryOpKind::DivAssign, $bump);
                let andeq = |lhs, rhs| binop(lhs, rhs, BinaryOpKind::BitAndAssign, $bump);
                let oreq = |lhs, rhs| binop(lhs, rhs, BinaryOpKind::BitOrAssign, $bump);
                let xoreq = |lhs, rhs| binop(lhs, rhs, BinaryOpKind::XorAssign, $bump);
                let modeq = |lhs, rhs| binop(lhs, rhs, BinaryOpKind::ModAssign, $bump);
                let assign = |lhs, rhs| binop(lhs, rhs, BinaryOpKind::Assign, $bump);

                let prefix_increment = |expr| prefix_op(expr, PrefixOpKind::Increment, $bump);
                let prefix_decrement = |expr| prefix_op(expr, PrefixOpKind::Decrement, $bump);
                let unary_plus = |expr| prefix_op(expr, PrefixOpKind::UnaryPlus, $bump);
                let unary_minus = |expr| prefix_op(expr, PrefixOpKind::UnaryMinus, $bump);
                let addr_or = |expr| prefix_op(expr, PrefixOpKind::AddressOf, $bump);
                let dereference = |expr| prefix_op(expr, PrefixOpKind::Dereference, $bump);
                let not = |expr| prefix_op(expr, PrefixOpKind::Not, $bump);
                let bitnot = |expr| prefix_op(expr, PrefixOpKind::BitNot, $bump);

                let suffix_increment = |expr| postfix_op(expr, PostfixOpKind::Increment, $bump);
                let suffix_decrement = |expr| postfix_op(expr, PostfixOpKind::Decrement, $bump);
                let a = || a($bump);
                let b = || b($bump);
                let c = || c($bump);
                let d = || d($bump);
                let e = || e($bump);
                let f = || f($bump);
                let g = || g($bump);
                (
                    ptr,
                    struct_,
                    array,
                    func_ptr,
                    cast,
                    binop,
                    prefix_op,
                    postfix_op,
                    ternary,
                    func_call,
                    member_access,
                    pointer_member_access,
                    array_index,
                    sizeof,
                    nullptr,
                    get_type,
                    add,
                    sub,
                    mul,
                    div,
                    eq,
                    gt,
                    lt,
                    geq,
                    leq,
                    noteq,
                    and,
                    or,
                    bitand,
                    bitor,
                    xor,
                    mod_,
                    addeq,
                    subeq,
                    muleq,
                    diveq,
                    andeq,
                    oreq,
                    xoreq,
                    modeq,
                    assign,
                    prefix_increment,
                    prefix_decrement,
                    unary_plus,
                    unary_minus,
                    addr_or,
                    dereference,
                    not,
                    bitnot,
                    suffix_increment,
                    suffix_decrement,
                    a,
                    b,
                    c,
                    d,
                    e,
                    f,
                    g,
                )
            }};
        }
    }
    #[test]
    fn test_type_parsing_valid() {
        use utils::*;
        let bump = &Bump::new();
        #[allow(unused)]
        let (
            ptr,
            struct_,
            array,
            func_ptr,
            cast,
            binop,
            prefix_op,
            postfix_op,
            ternary,
            func_call,
            member_access,
            pointer_member_access,
            array_index,
            sizeof,
            nullptr,
            get_type,
            add,
            sub,
            mul,
            div,
            eq,
            gt,
            lt,
            geq,
            leq,
            noteq,
            and,
            or,
            bitand,
            bitor,
            xor,
            mod_,
            addeq,
            subeq,
            muleq,
            diveq,
            andeq,
            oreq,
            xoreq,
            modeq,
            assign,
            prefix_increment,
            prefix_decrement,
            unary_plus,
            unary_minus,
            addr_or,
            dereference,
            not,
            bitnot,
            suffix_increment,
            suffix_decrement,
            a,
            b,
            c,
            d,
            e,
            f,
            g,
        ) = anchor!(bump);
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
        compare_types("Something", struct_("Something"));
        compare_types("[Something; 1]", array(struct_("Something"), 1));
        compare_types("[int; 2555555]", array(int(), 2555555));
        compare_types("**Something", ptr(ptr(struct_("Something"))));
        compare_types("fn()", func_ptr(Vec::new(), None));
        compare_types("*fn()", ptr(func_ptr(Vec::new(), None)));
        compare_types("fn(int)", func_ptr(vec![int()], None));
        compare_types(
            "fn(int, int) -> int",
            func_ptr(vec![int(), int()], Some(int())),
        );
        compare_types(
            "fn(int, int) -> SomeStruct",
            func_ptr(vec![int(), int()], Some(struct_("SomeStruct"))),
        );
        compare_types(
            "fn(int, int) -> ***SomeStruct",
            func_ptr(
                vec![int(), int()],
                Some(ptr(ptr(ptr(struct_("SomeStruct"))))),
            ),
        );
        compare_types(
            "fn(SomeStruct, SomeOtherStruct) -> SomeStruct",
            func_ptr(
                vec![struct_("SomeStruct"), struct_("SomeOtherStruct")],
                Some(struct_("SomeStruct")),
            ),
        );
        compare_types(
            "fn(int, int) -> fn(int, int) -> int",
            func_ptr(
                vec![int(), int()],
                Some(func_ptr(vec![int(), int()], Some(int()))),
            ),
        );
        compare_types(
            "fn(fn(int, int) -> fn(int, int) -> int, fn(fn(int, int) -> fn(int, int) -> int, int) -> fn(int, int) -> int) -> fn(int, int) -> int",
            func_ptr(
                vec![
                    func_ptr(
                        vec![int(), int()],
                        Some(func_ptr(vec![int(), int()], Some(int()))),
                    ),
                    func_ptr(
                        vec![
                            func_ptr(
                                vec![int(), int()],
                                Some(func_ptr(vec![int(), int()], Some(int()))),
                            ),
                            int(),
                        ],
                        Some(func_ptr(vec![int(), int()], Some(int()))),
                    ),
                ],
                Some(func_ptr(vec![int(), int()], Some(int()))),
            ),
        );
    }

    #[test]
    fn test_type_parsing_fail() {
        use utils::*;
        let bump = &Bump::new();
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
            let tokens = lex(s);
            let mut parser = Parser::new(&tokens);
            let res = parser.parse_type(bump).is_err();
            assert!(res || !parser.is_at_end() || !parser.errors.is_empty());
        }
    }

    #[track_caller]
    fn compare_exprs(s: &str, expected: Expr<'_>) {
        use utils::*;
        let bump = Bump::new();
        let tokens = lex(s);
        let mut parser = Parser::new(&tokens);
        let parsed = parser.parse_expr(&bump).expect("Should parse correctly");
        assert_eq!(parsed, expected);
    }

    #[test]
    fn test_expr_parsing_valid() {
        use utils::*;
        let bump = &Bump::new();
        #[allow(unused)]
        let (
            ptr,
            struct_,
            array,
            func_ptr,
            cast,
            binop,
            prefix_op,
            postfix_op,
            ternary,
            func_call,
            member_access,
            pointer_member_access,
            array_index,
            sizeof,
            nullptr,
            get_type,
            add,
            sub,
            mul,
            div,
            eq,
            gt,
            lt,
            geq,
            leq,
            noteq,
            and,
            or,
            bitand,
            bitor,
            xor,
            mod_,
            addeq,
            subeq,
            muleq,
            diveq,
            andeq,
            oreq,
            xoreq,
            modeq,
            assign,
            prefix_increment,
            prefix_decrement,
            unary_plus,
            unary_minus,
            addr_of,
            dereference,
            not,
            bitnot,
            suffix_increment,
            suffix_decrement,
            a,
            b,
            c,
            d,
            e,
            f,
            g,
        ) = anchor!(bump);
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
        compare_exprs("c.a", member_access(c(), "a"));
        compare_exprs("c->a", pointer_member_access(c(), "a"));
        compare_exprs("a[b]", array_index(a(), b()));
        compare_exprs("sizeof(int)", sizeof(get_type("int")));
        compare_exprs("sizeof(**int)", sizeof(get_type("**int")));
        compare_exprs("sizeof(a)", sizeof(get_type("a")));
        compare_exprs("*a", dereference(a()));
        compare_exprs("nullptr", nullptr());
        // Should fail in type checking, but parser should allow it
        compare_exprs("*nullptr", dereference(nullptr()));

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
            add(member_access(c(), "a"), pointer_member_access(b(), "c")),
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
                            cast(pointer_member_access(b(), "c"), get_type("int")),
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
        let bump = &Bump::new();
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
        ];
        for s in failing_tests {
            let lexed = lex(s);
            let mut parser = Parser::new(&lexed);
            assert!(
                parser.parse_expr(bump).is_err()
                    || !parser.is_at_end()
                    || !parser.errors.is_empty()
            );
        }
    }
}
