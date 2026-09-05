use std::{iter::Peekable, num::IntErrorKind, str::CharIndices};

use smol_str::SmolStr;

use crate::common::symbol::Symbol;
use crate::{common::span::Span, syntax::context::Context};

#[derive(Clone, Debug, PartialEq, Copy)]
pub enum KeywordKind {
    As,
    Nullptr,
    Int,
    Return,
    If,
    Else,
    While,
    For,
    Sizeof,
    Struct,
    Fn,
    FnSys,
    Break,
    Continue,
    Void,
    Let,
    NoAlias,
    Assert,
    NewProvenance,
    ExposeProvenance,
    UnexposeProv,
    CopyProvenance,
}
#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    Keyword(KeywordKind),
    Ident(Symbol),
    IntLiteral(u64, u32),
    Plus,
    Minus,
    Asterisk,
    Slash,
    LParen,
    RParen,
    LCurly,
    RCurly,
    LSquare,
    RSquare,
    Semicolon,
    Comma,
    Equal,
    DoubleEqual,
    GreaterThan,
    LessThan,
    ExclamationMarkEqual,
    GreaterOrEqual,
    LessOrEqual,
    DoubleAmpersand,
    Ampersand,
    DoublePipe,
    Pipe,
    Caret,
    ExclamationMark,
    QuestionMark,
    Colon,
    Period,
    Percent,
    PlusEqual,
    MinusEqual,
    AsteriskEqual,
    SlashEqual,
    DoublePlus,
    DoubleMinus,
    AmpersandEqual,
    PipeEqual,
    CaretEqual,
    PercentEqual,
    Arrow,
    Error,
    Tilde,
    At,
    Shl,
    Shr,
    ShlEquals,
    ShrEquals,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }
}

#[derive(Clone, Debug)]
pub enum LexingError {
    UnexpectedEndOfInput,
    UnknownCharacters { characters: SmolStr, span: Span },
    InvalidNumberCharacter { character: char, span: Span },
    EmptyNumber { span: Span },
    InvalidDigitInNumber { span: Span },
    IntegerTooLarge { span: Span },
}

#[derive(Debug)]
pub struct Lexer<'a> {
    tokens: Vec<Token>,
    errors: Vec<LexingError>,
    input: &'a str,
    iter: Peekable<CharIndices<'a>>,
}

#[derive(Clone, Debug)]
pub struct LexingOutput {
    pub tokens: Vec<Token>,
    pub errors: Vec<LexingError>,
}

impl LexingOutput {
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

#[derive(Clone, Debug, Copy, PartialEq, Eq, Hash)]
enum CharType {
    Alpha,
    Num,
    Whitespace,
    Punctutation,
    Bracket,
    Unknown,
}

impl CharType {
    fn of(c: char) -> Self {
        match c {
            'a'..='z' | 'A'..='Z' | '_' => CharType::Alpha,
            '0'..='9' => CharType::Num,
            ws if ws.is_whitespace() => CharType::Whitespace,
            '+' | '-' | '*' | '/' | ';' | ',' | '=' | '>' | '<' | '!' | '&' | '|' | '^' | '?'
            | ':' | '.' | '%' | '~' | '@' => CharType::Punctutation,
            '(' | ')' | '{' | '}' | '[' | ']' => CharType::Bracket,
            _ => CharType::Unknown,
        }
    }
}

impl<'a> Lexer<'a> {
    pub fn lex(s: &str, ctx: &mut Context) -> LexingOutput {
        let lexer = Lexer::new(s);
        lexer.lex_inner(ctx)
    }
    fn new(input: &'a str) -> Self {
        assert!(input.len() <= 4_000_000_000);
        Self {
            tokens: Vec::new(),
            errors: Vec::new(),
            input,
            iter: input.char_indices().peekable(),
        }
    }
    fn curr_index(&mut self) -> u32 {
        self.iter
            .peek()
            .map(|(idx, _c)| *idx)
            .unwrap_or(self.input.len()) as u32
    }
    fn handle_ident(&mut self, _c: char, start: u32, ctx: &mut Context) {
        while let Some((_idx, c2)) = self.iter.peek()
            && (c2.is_ascii_alphanumeric() || *c2 == '_')
        {
            self.iter.next();
        }
        let end = self.curr_index();
        let s = &self.input[start as usize..end as usize];
        let span = Span::new(start, end);
        let kind = match s {
            "nullptr" => TokenKind::Keyword(KeywordKind::Nullptr),
            "as" => TokenKind::Keyword(KeywordKind::As),
            "int" => TokenKind::Keyword(KeywordKind::Int),
            "return" => TokenKind::Keyword(KeywordKind::Return),
            "if" => TokenKind::Keyword(KeywordKind::If),
            "else" => TokenKind::Keyword(KeywordKind::Else),
            "while" => TokenKind::Keyword(KeywordKind::While),
            "for" => TokenKind::Keyword(KeywordKind::For),
            "sizeof" => TokenKind::Keyword(KeywordKind::Sizeof),
            "struct" => TokenKind::Keyword(KeywordKind::Struct),
            "fn" => TokenKind::Keyword(KeywordKind::Fn),
            "break" => TokenKind::Keyword(KeywordKind::Break),
            "continue" => TokenKind::Keyword(KeywordKind::Continue),
            "void" => TokenKind::Keyword(KeywordKind::Void),
            "let" => TokenKind::Keyword(KeywordKind::Let),
            "noalias" => TokenKind::Keyword(KeywordKind::NoAlias),
            "assert" => TokenKind::Keyword(KeywordKind::Assert),
            "new_prov" => TokenKind::Keyword(KeywordKind::NewProvenance),
            "expose_prov" => TokenKind::Keyword(KeywordKind::ExposeProvenance),
            "unexpose_prov" => TokenKind::Keyword(KeywordKind::UnexposeProv),
            "copy_prov" => TokenKind::Keyword(KeywordKind::CopyProvenance),
            "fn_sys" => TokenKind::Keyword(KeywordKind::FnSys),
            _ => TokenKind::Ident(ctx.intern_symbol(s)),
        };
        self.tokens.push(Token { kind, span });
    }
    fn handle_number(&mut self, c: char, start: u32) {
        let (radix, offset) = match self.iter.peek() {
            Some((_, 'x')) | Some((_, 'X')) if c == '0' => {
                self.iter.next();
                (16, 2)
            }
            Some((_, 'b')) | Some((_, 'B')) if c == '0' => {
                self.iter.next();
                (2, 2)
            }
            None => {
                let span = Span::new(start, self.curr_index());
                let Some(num) = c.to_digit(10) else {
                    unreachable!();
                };
                self.tokens
                    .push(Token::new(TokenKind::IntLiteral(num as u64, 10), span));
                return;
            }
            _ => (10, 0),
        };
        while let Some((_, char)) = self.iter.peek()
            && char.is_ascii_alphanumeric()
        {
            self.iter.next();
        }
        let end = self.curr_index();
        let s = &self.input[start as usize + offset..end as usize];
        let span = Span::new(start, end);
        let num = match u64::from_str_radix(s, radix) {
            Ok(n) => n,
            Err(e) => {
                self.tokens.push(Token::new(TokenKind::Error, span));
                let err = match *e.kind() {
                    IntErrorKind::Empty => LexingError::EmptyNumber { span },
                    IntErrorKind::InvalidDigit => LexingError::InvalidDigitInNumber { span },
                    IntErrorKind::PosOverflow => LexingError::IntegerTooLarge { span },
                    _ => unreachable!(),
                };
                self.errors.push(err);
                return;
            }
        };
        self.tokens
            .push(Token::new(TokenKind::IntLiteral(num, radix), span));
    }
    fn handle_operator(&mut self, c: char, start: u32) {
        let kind = match c {
            '+' => match self.iter.peek() {
                Some((_, '+')) => {
                    self.iter.next();
                    TokenKind::DoublePlus
                }
                Some((_, '=')) => {
                    self.iter.next();
                    TokenKind::PlusEqual
                }
                _ => TokenKind::Plus,
            },
            '-' => match self.iter.peek() {
                Some((_, '-')) => {
                    self.iter.next();
                    TokenKind::DoubleMinus
                }
                Some((_, '=')) => {
                    self.iter.next();
                    TokenKind::MinusEqual
                }
                Some((_, '>')) => {
                    self.iter.next();
                    TokenKind::Arrow
                }
                _ => TokenKind::Minus,
            },
            '*' => match self.iter.peek() {
                Some((_, '=')) => {
                    self.iter.next();
                    TokenKind::AsteriskEqual
                }
                _ => TokenKind::Asterisk,
            },
            '/' => match self.iter.peek() {
                Some((_, '=')) => {
                    self.iter.next();
                    TokenKind::SlashEqual
                }
                _ => TokenKind::Slash,
            },
            '=' => match self.iter.peek() {
                Some((_, '=')) => {
                    self.iter.next();
                    TokenKind::DoubleEqual
                }
                _ => TokenKind::Equal,
            },
            '>' => match self.iter.peek() {
                Some((_, '=')) => {
                    self.iter.next();
                    TokenKind::GreaterOrEqual
                }
                Some((_, '>')) => {
                    self.iter.next();
                    if let Some((_, '=')) = self.iter.peek() {
                        self.iter.next();
                        TokenKind::ShrEquals
                    } else {
                        TokenKind::Shr
                    }
                }
                _ => TokenKind::GreaterThan,
            },
            '<' => match self.iter.peek() {
                Some((_, '=')) => {
                    self.iter.next();
                    TokenKind::LessOrEqual
                }
                Some((_, '<')) => {
                    self.iter.next();
                    if let Some((_, '=')) = self.iter.peek() {
                        self.iter.next();
                        TokenKind::ShlEquals
                    } else {
                        TokenKind::Shl
                    }
                }
                _ => TokenKind::LessThan,
            },
            '!' => match self.iter.peek() {
                Some((_, '=')) => {
                    self.iter.next();
                    TokenKind::ExclamationMarkEqual
                }
                _ => TokenKind::ExclamationMark,
            },
            '&' => match self.iter.peek() {
                Some((_, '&')) => {
                    self.iter.next();
                    TokenKind::DoubleAmpersand
                }
                Some((_, '=')) => {
                    self.iter.next();
                    TokenKind::AmpersandEqual
                }
                _ => TokenKind::Ampersand,
            },
            '|' => match self.iter.peek() {
                Some((_, '|')) => {
                    self.iter.next();
                    TokenKind::DoublePipe
                }
                Some((_, '=')) => {
                    self.iter.next();
                    TokenKind::PipeEqual
                }
                _ => TokenKind::Pipe,
            },
            '^' => match self.iter.peek() {
                Some((_, '=')) => {
                    self.iter.next();
                    TokenKind::CaretEqual
                }
                _ => TokenKind::Caret,
            },
            '?' => TokenKind::QuestionMark,
            ':' => TokenKind::Colon,
            '.' => TokenKind::Period,
            '%' => match self.iter.peek() {
                Some((_, '=')) => {
                    self.iter.next();
                    TokenKind::PercentEqual
                }
                _ => TokenKind::Percent,
            },
            ';' => TokenKind::Semicolon,
            ',' => TokenKind::Comma,
            '~' => TokenKind::Tilde,
            '@' => TokenKind::At,
            _ => unreachable!(),
        };
        let span = Span::new(start, self.curr_index());
        self.tokens.push(Token::new(kind, span));
    }
    fn handle_brackets(&mut self, c: char, start: u32) {
        let token = match c {
            '(' => TokenKind::LParen,
            ')' => TokenKind::RParen,
            '{' => TokenKind::LCurly,
            '}' => TokenKind::RCurly,
            '[' => TokenKind::LSquare,
            ']' => TokenKind::RSquare,
            _ => unreachable!(),
        };
        self.tokens.push(Token {
            kind: token,
            span: Span::new(start, start + 1),
        })
    }
    fn lex_inner(mut self, ctx: &mut Context) -> LexingOutput {
        while let Some((index, curr)) = self.iter.next() {
            if curr == '/'
                && let Some((_index, next)) = self.iter.peek()
                && *next == '/'
            {
                self.iter.next();
                while let Some((_index, c)) = self.iter.peek()
                    && *c != '\n'
                {
                    self.iter.next();
                }
                self.iter.next();
                continue;
            }
            match CharType::of(curr) {
                CharType::Alpha => self.handle_ident(curr, index as u32, ctx),
                CharType::Num => self.handle_number(curr, index as u32),
                CharType::Whitespace => {}
                CharType::Punctutation => self.handle_operator(curr, index as u32),
                CharType::Bracket => self.handle_brackets(curr, index as u32),
                CharType::Unknown => {
                    while let Some((_index, c)) = self.iter.peek()
                        && CharType::of(*c) == CharType::Unknown
                    {
                        self.iter.next();
                    }
                    let end = self.curr_index();
                    let span = Span::new(index as u32, end);
                    self.errors.push(LexingError::UnknownCharacters {
                        characters: SmolStr::new(&self.input[index..end as usize]),
                        span,
                    });
                    self.tokens.push(Token {
                        kind: TokenKind::Error,
                        span,
                    });
                }
            }
        }
        LexingOutput {
            tokens: self.tokens,
            errors: self.errors,
        }
    }
}

#[test]
fn test_lexer() {
    #[track_caller]
    fn test(s: &str, expect: TokenKind) {
        let mut ctx = Context::new();
        let output = Lexer::lex(s, &mut ctx);
        assert!(!output.has_errors());
        assert_eq!(output.tokens.len(), 1);
        assert_eq!(output.tokens[0].kind, expect);
    }
    #[track_caller]
    fn test_ident(s: &str) {
        let mut ctx = Context::new();
        let output = Lexer::lex(s, &mut ctx);
        assert!(!output.has_errors());
        assert_eq!(output.tokens.len(), 1);
        assert_eq!(
            output.tokens[0].kind,
            TokenKind::Ident(ctx.intern_symbol(s))
        );
    }
    test("nullptr", TokenKind::Keyword(KeywordKind::Nullptr));
    test("as", TokenKind::Keyword(KeywordKind::As));
    test("int", TokenKind::Keyword(KeywordKind::Int));
    test("return", TokenKind::Keyword(KeywordKind::Return));
    test("if", TokenKind::Keyword(KeywordKind::If));
    test("else", TokenKind::Keyword(KeywordKind::Else));
    test("while", TokenKind::Keyword(KeywordKind::While));
    test("for", TokenKind::Keyword(KeywordKind::For));
    test("sizeof", TokenKind::Keyword(KeywordKind::Sizeof));
    test("struct", TokenKind::Keyword(KeywordKind::Struct));
    test("fn", TokenKind::Keyword(KeywordKind::Fn));
    test("break", TokenKind::Keyword(KeywordKind::Break));
    test("continue", TokenKind::Keyword(KeywordKind::Continue));
    test("void", TokenKind::Keyword(KeywordKind::Void));
    test("let", TokenKind::Keyword(KeywordKind::Let));
    test("noalias", TokenKind::Keyword(KeywordKind::NoAlias));
    test("assert", TokenKind::Keyword(KeywordKind::Assert));

    test("new_prov", TokenKind::Keyword(KeywordKind::NewProvenance));
    test(
        "expose_prov",
        TokenKind::Keyword(KeywordKind::ExposeProvenance),
    );
    test(
        "unexpose_prov",
        TokenKind::Keyword(KeywordKind::UnexposeProv),
    );
    test("copy_prov", TokenKind::Keyword(KeywordKind::CopyProvenance));
    test("fn_sys", TokenKind::Keyword(KeywordKind::FnSys));

    test_ident("hello");
    test_ident("hi");
    test_ident("interesting_thing");
    test_ident("_");
    test_ident("_______1521521512512");

    test("0", TokenKind::IntLiteral(0, 10));
    test("1", TokenKind::IntLiteral(1, 10));
    test("1000000000", TokenKind::IntLiteral(1000000000, 10));
    test(
        "000000000000000000000000000000000000000000000000000000000000000000",
        TokenKind::IntLiteral(0, 10),
    );
    test(
        "18446744073709551615",
        TokenKind::IntLiteral(18446744073709551615, 10),
    );
    test(&u64::MAX.to_string(), TokenKind::IntLiteral(u64::MAX, 10));
    test("0x0", TokenKind::IntLiteral(0, 16));
    test("0x1", TokenKind::IntLiteral(1, 16));
    test(
        "0xFFFFFFFFFFFFFFFF",
        TokenKind::IntLiteral(0xFFFFFFFFFFFFFFFF, 16),
    );
    test("0b0", TokenKind::IntLiteral(0, 2));
    test("0b1", TokenKind::IntLiteral(1, 2));
    test("0b10", TokenKind::IntLiteral(2, 2));
    test(
        "0b0000000011111111000000001111111100000000111111110000000011111111",
        TokenKind::IntLiteral(
            0b0000000011111111000000001111111100000000111111110000000011111111,
            2,
        ),
    );

    test("+", TokenKind::Plus);
    test("-", TokenKind::Minus);
    test("*", TokenKind::Asterisk);
    test("/", TokenKind::Slash);
    test("(", TokenKind::LParen);
    test(")", TokenKind::RParen);
    test("{", TokenKind::LCurly);
    test("}", TokenKind::RCurly);
    test("[", TokenKind::LSquare);
    test("]", TokenKind::RSquare);
    test(";", TokenKind::Semicolon);
    test(",", TokenKind::Comma);
    test("=", TokenKind::Equal);
    test("==", TokenKind::DoubleEqual);
    test(">", TokenKind::GreaterThan);
    test("<", TokenKind::LessThan);
    test("!=", TokenKind::ExclamationMarkEqual);
    test(">=", TokenKind::GreaterOrEqual);
    test("<=", TokenKind::LessOrEqual);
    test("&&", TokenKind::DoubleAmpersand);
    test("&", TokenKind::Ampersand);
    test("||", TokenKind::DoublePipe);
    test("|", TokenKind::Pipe);
    test("^", TokenKind::Caret);
    test("!", TokenKind::ExclamationMark);
    test("?", TokenKind::QuestionMark);
    test(":", TokenKind::Colon);
    test(".", TokenKind::Period);
    test("%", TokenKind::Percent);
    test("+=", TokenKind::PlusEqual);
    test("-=", TokenKind::MinusEqual);
    test("*=", TokenKind::AsteriskEqual);
    test("/=", TokenKind::SlashEqual);
    test("++", TokenKind::DoublePlus);
    test("--", TokenKind::DoubleMinus);
    test("&=", TokenKind::AmpersandEqual);
    test("|=", TokenKind::PipeEqual);
    test("^=", TokenKind::CaretEqual);
    test("%=", TokenKind::PercentEqual);
    test("->", TokenKind::Arrow);
    test("~", TokenKind::Tilde);
    test("@", TokenKind::At);
    test("<<", TokenKind::Shl);
    test(">>", TokenKind::Shr);
    test("<<=", TokenKind::ShlEquals);
    test(">>=", TokenKind::ShrEquals);
}
