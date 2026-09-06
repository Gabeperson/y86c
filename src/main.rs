use y86c::analysis::ast_validator::*;
use y86c::analysis::symbol_table::SymbolTableBuilder;
use y86c::analysis::type_checker::TypeChecker;
use y86c::ir::interp::IrInterpreter;
use y86c::ir::lower::Lowerer;
use y86c::ir::lower_prepass::LoweringPrepass;
use y86c::ir::print::IrPrinter;
use y86c::syntax::context::Context;
use y86c::syntax::lexer::Lexer;
use y86c::syntax::parser::Parser;
fn main() {
    let file = std::fs::read_to_string("file.y86").unwrap();
    let mut ctx = Context::new();
    let lexed = Lexer::lex(&file, &mut ctx);
    let mut has_err = false;
    if lexed.has_errors() {
        dbg!(lexed.errors);
        has_err = true;
    }
    let parsed = Parser::parse_test(&lexed.tokens, &mut ctx);
    if parsed.has_errors() {
        dbg!(parsed.errors);
        has_err = true;
    }
    let program = parsed.program;
    let validation_errs = AstValidator::validate(&program, true, &ctx);
    if !validation_errs.is_empty() {
        dbg!(validation_errs);
        has_err = true;
    }
    let symbol_res = SymbolTableBuilder::build(&program, &mut ctx);
    if !symbol_res.errors.is_empty() {
        dbg!(symbol_res.errors);
        has_err = true;
    }
    let symbol_table = symbol_res.symbol_table;
    let type_check_res = TypeChecker::check(&mut ctx, &symbol_table, &program);
    if !type_check_res.errors.is_empty() {
        dbg!(type_check_res.errors);
        has_err = true;
    }
    if has_err {
        std::process::exit(1);
    }
    let type_table = type_check_res.type_table;
    let mut prepass = LoweringPrepass::run(&program, &ctx, &symbol_table);
    let lowerer = Lowerer::new(&mut prepass, &symbol_table, &mut ctx, &type_table);
    let ir_program = match lowerer.lower(&program) {
        Ok(p) => p,
        Err(e) => {
            dbg!(e);
            std::process::exit(1);
        }
    };
    let printer = IrPrinter::new(&ir_program.typectx, &ctx.symbol_interner);
    for function in ir_program.functions.values() {
        let res = printer.print(function).unwrap();
        println!("{res}");
        println!();
    }

    let mut interp = IrInterpreter::new(&ir_program, &file, &ctx);
    interp.run();
    println!("Interpreter successfully ran");
}
