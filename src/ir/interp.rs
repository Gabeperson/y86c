use ahash::AHashMap;

use crate::{common::symbol::Symbol, ir::repr::IrProgram};

pub struct GlobalVariableProvider {}

#[derive(Debug, Clone)]
pub struct IrInterpreter<'a> {
    program: &'a IrProgram,
    buf: Vec<u8>,
}

impl IrInterpreter<'_> {
    fn prep(&mut self) {
        let mut map = AHashMap::new();
        let mut pos = 0usize;
        for (name, global) in self.program.globals.map.iter() {
            let layout = global.val.layout();
            pos = pos.next_multiple_of(layout.align);
            map.insert(*name, (pos, layout.size));
            pos += layout.size;
        }
        self.buf.resize(pos, 0);
        for (name, global) in self.program.globals.map.iter() {
            let (pos, size) = map[name];
            let buf = &mut self.buf[pos..][..size];
            global.val.write(buf, |sym| {
                if let Some(&(pos, _)) = map.get(&sym) {
                    return pos as i64;
                }
                if let Some(idx) = self.program.functions.get_index_of(&sym) {
                    return idx as i64;
                }
                unreachable!()
            });
        }
    }
    fn run_func(&mut self, func: Symbol, args: &[i64]) -> Option<i64> {
        let func = self.program.functions.get(&func).unwrap();
        let mut stackslot_locs = Vec::with_capacity(func.stack_slots.len());
        let stackslots_size = func.stack_slots.fold(0usize, |pos, id, stackslot| {
            let pos: usize = pos.next_multiple_of(stackslot.align as usize);
            assert_eq!(id.get() as usize, stackslot_locs.len());
            stackslot_locs.push(pos);
            pos + stackslot.size as usize
        });
        let stack_start = self.buf.len();
        self.buf.resize(stack_start + stackslots_size, 0);
        let mut block_id = func.entry;
        let mut inst_idx = 0;
        loop {
            let block = func.blocks.get(block_id);
            let inst_id = block.insts[inst_idx];
            let inst = func.insts.get(inst_id);
        }
    }
}
