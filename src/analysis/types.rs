use std::collections::hash_map::Entry;

use crate::syntax::ast::{Type as AstType, TypeKind};
use ahash::AHashMap;
use smol_str::SmolStr;
use tinyvec::TinyVec;

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Default)]
pub struct TypeId(u32);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    Void,
    Int,
    Ptr {
        pointee: TypeId,
        noalias: bool,
    },
    Struct {
        name: SmolStr,
    },
    Array {
        element_type: TypeId,
        len: i64,
    },
    FuncPtr {
        return_type: Option<TypeId>,
        param_types: TinyVec<[TypeId; 5]>,
    },
}

#[derive(Clone, Debug)]
pub struct TypeArena {
    map: AHashMap<Type, TypeId>,
    arr: Vec<Type>,
}
impl Default for TypeArena {
    fn default() -> Self {
        Self::new()
    }
}
impl TypeArena {
    pub fn new() -> Self {
        Self {
            map: AHashMap::new(),
            arr: Vec::new(),
        }
    }

    pub fn intern_ast_type(&mut self, typ: &AstType<'_>) -> TypeId {
        match typ.inner {
            TypeKind::Void => self.intern_type(Type::Void),
            TypeKind::Int => self.intern_type(Type::Int),
            TypeKind::Ptr { pointee, noalias } => {
                let pointee = self.intern_ast_type(pointee);
                self.intern_type(Type::Ptr {
                    pointee,
                    noalias: *noalias,
                })
            }
            TypeKind::Struct { name } => self.intern_type(Type::Struct {
                name: SmolStr::new(name.ident),
            }),
            TypeKind::Array { element_type, size } => {
                let element_type = self.intern_ast_type(element_type);
                self.intern_type(Type::Array {
                    element_type,
                    len: *size,
                })
            }
            TypeKind::FuncPtr {
                return_type,
                param_types,
            } => {
                let return_type = return_type.as_ref().map(|t| self.intern_ast_type(t));
                let param_types = param_types
                    .iter()
                    .map(|t| self.intern_ast_type(t))
                    .collect();
                self.intern_type(Type::FuncPtr {
                    return_type,
                    param_types,
                })
            }
            TypeKind::Error => panic!("Internal Compiler Error"),
        }
    }

    pub fn intern_type(&mut self, typ: Type) -> TypeId {
        match self.map.entry(typ) {
            Entry::Occupied(entry) => *entry.get(),
            Entry::Vacant(entry) => {
                let id = TypeId(self.arr.len() as u32);
                self.arr.push(entry.key().clone());
                entry.insert(id);
                id
            }
        }
    }
}
