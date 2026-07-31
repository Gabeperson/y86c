use std::{
    iter::Peekable,
    num::{IntErrorKind, ParseIntError},
    str::CharIndices,
};

use smol_str::SmolStr;

use crate::{
    arena::{InternedString, StringInterner, Symbol},
    span::Span,
};

#[derive(Clone, Debug)]
pub enum KeywordKind {
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
    Break,
    Continue,
    Void,
}
#[derive(Clone, Debug)]
pub enum TokenKind {
    Keyword(KeywordKind),
    Ident(InternedString),
    IntLiteral { value: u64 },
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
}

#[derive(Clone, Debug)]
pub struct Token {
    kind: TokenKind,
    span: Span,
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
    interner: &'a mut StringInterner,
}

#[derive(Clone, Debug)]
pub struct LexingOutput {
    pub tokens: Vec<Token>,
    pub errors: Vec<LexingError>,
}

impl LexingOutput {
    fn has_errors(&self) -> bool {
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
            | ':' | '.' | '%' => CharType::Punctutation,
            '(' | ')' | '{' | '}' | '[' | ']' => CharType::Bracket,
            _ => CharType::Unknown,
        }
    }
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str, interner: &'a mut StringInterner) -> Self {
        assert!(input.len() <= 4_000_000_000);
        Self {
            tokens: Vec::new(),
            errors: Vec::new(),
            input,
            iter: input.char_indices().peekable(),
            interner,
        }
    }
    fn curr_index(&mut self) -> u32 {
        self.iter
            .peek()
            .map(|(idx, _c)| *idx)
            .unwrap_or(self.input.len()) as u32
    }
    fn handle_ident(&mut self, _c: char, start: u32) {
        while let Some((_idx, c2)) = self.iter.peek()
            && (c2.is_ascii_alphanumeric() || *c2 == '_')
        {
            self.iter.next();
        }
        let end = self.curr_index();
        let s = &self.input[start as usize..end as usize];
        let span = Span::new(start, end as u32);
        let kind = match s {
            "nullptr" => TokenKind::Keyword(KeywordKind::Nullptr),
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
            _ => {
                let interned = self.interner.intern(Symbol::new(s), span);
                TokenKind::Ident(interned)
            }
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
                self.tokens.push(Token::new(
                    TokenKind::IntLiteral { value: num as u64 },
                    span,
                ));
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
            .push(Token::new(TokenKind::IntLiteral { value: num }, span));
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
                Some((_, '/')) => {
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
                _ => TokenKind::GreaterThan,
            },
            '<' => match self.iter.peek() {
                Some((_, '=')) => {
                    self.iter.next();
                    TokenKind::LessOrEqual
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
    pub fn lex(mut self) -> LexingOutput {
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
                CharType::Alpha => self.handle_ident(curr, index as u32),
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
        // while !self.is_eof() {
        //     let curr = self.current();
        //     if curr == "/" && self.can_doubleparse() && self.get(2) == "//" {
        //         // comment
        //         self.advancen(2);
        //     }
        // }
        todo!()
    }
}
