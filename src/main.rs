use y86c::lexer::Lexer;

fn main() {
    let s = std::fs::read_to_string("testfiles/lexertest.txt").unwrap();
    let mut string_interner = 
    let lexer = Lexer::new(&s);
}
