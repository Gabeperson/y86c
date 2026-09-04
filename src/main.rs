use y86c::analysis::ast_validator::*;
use y86c::analysis::symbol_table::SymbolTableBuilder;
use y86c::analysis::type_checker::TypeChecker;
use y86c::ir::globals::GlobalEvaluator;
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
    if lexed.has_errors() {
        dbg!(lexed.errors);
        std::process::exit(1);
    }
    let parsed = Parser::parse_test(&lexed.tokens, &mut ctx);
    if parsed.has_errors() {
        dbg!(parsed.errors);
        std::process::exit(1);
    }
    let program = parsed.program;
    let validation_errs = AstValidator::validate(&program, true, &ctx);
    if !validation_errs.is_empty() {
        dbg!(validation_errs);
        std::process::exit(1);
    }
    let symbol_res = SymbolTableBuilder::build(&program, &mut ctx);
    if !symbol_res.errors.is_empty() {
        dbg!(symbol_res.errors);
        std::process::exit(1);
    }
    let symbol_table = symbol_res.symbol_table;
    let type_check_res = TypeChecker::check(&mut ctx, &symbol_table, &program);
    if !type_check_res.errors.is_empty() {
        dbg!(type_check_res.errors);
        std::process::exit(1);
    }
    let type_table = type_check_res.type_table;
    let mut prepass = LoweringPrepass::run(&program, &ctx, &symbol_table);
    let lowerer = Lowerer::new(&mut prepass, &symbol_table, &mut ctx, &type_table);
    let (typectx, functions) = lowerer.lower(&program);

    let globals = match GlobalEvaluator::eval(&program, &ctx, &type_table, &symbol_table) {
        Ok(globals) => globals,
        Err(e) => {
            dbg!(e);
            std::process::exit(1);
        }
    };
    dbg!(globals);

    // let printer = IrPrinter::new(&typectx, &ctx.symbol_interner);
    // for function in functions {
    //     let res = printer.print(&function).unwrap();
    //     println!("{res}");
    //     println!();
    // }
}
