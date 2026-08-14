use crate::common::interner::define_arena;
use smol_str::SmolStr;
define_arena!(SmolStr, Symbol, SymbolArena; dedup);
