use bumpalo::Bump;
use y86c::{lexer::Lexer, parser::Parser};

fn main() {
    let s = std::fs::read_to_string("testfiles/main.y86").unwrap();
    let lexer = Lexer::new(&s);
    let res = lexer.lex();
    let parser = Parser::new(&res.tokens);
    let bump = Bump::new();
    let parsed = parser.parse(&bump);
    dbg!(parsed.errors);
}
