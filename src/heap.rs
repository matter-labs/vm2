use std::ops::{Index, IndexMut};

use zkevm_opcode_defs::system_params::NEW_FRAME_MEMORY_STIPEND;

#[derive(Copy, Clone, PartialEq, Debug)]
pub struct HeapId(u32);

impl HeapId {
    /// Only for dealing with external data structures, never use internally.
    pub(crate) fn from_u32_unchecked(value: u32) -> Self {
        Self(value)
    }

    pub(crate) fn to_u32(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone)]
pub struct Heaps(Vec<Vec<u8>>);

pub(crate) const CALLDATA_HEAP: HeapId = HeapId(1);
pub const FIRST_HEAP: HeapId = HeapId(2);
pub(crate) const FIRST_AUX_HEAP: HeapId = HeapId(3);

impl Heaps {
    pub(crate) fn new(calldata: Vec<u8>) -> Self {
        // The first heap can never be used because heap zero
        // means the current heap in precompile calls
        Self(vec![vec![], calldata, vec![], vec![]])
    }

    pub(crate) fn allocate(&mut self) -> HeapId {
        let id = HeapId(self.0.len() as u32);
        self.0.push(vec![0; NEW_FRAME_MEMORY_STIPEND as usize]);
        id
    }

    pub(crate) fn deallocate(&mut self, heap: HeapId) {
        self.0[heap.0 as usize] = vec![];
    }
}

impl Index<HeapId> for Heaps {
    type Output = Vec<u8>;

    fn index(&self, index: HeapId) -> &Self::Output {
        &self.0[index.0 as usize]
    }
}

impl IndexMut<HeapId> for Heaps {
    fn index_mut(&mut self, index: HeapId) -> &mut Self::Output {
        &mut self.0[index.0 as usize]
    }
}

impl PartialEq for Heaps {
    fn eq(&self, other: &Self) -> bool {
        for i in 0..self.0.len().max(other.0.len()) {
            if self.0.get(i).unwrap_or(&vec![]) != other.0.get(i).unwrap_or(&vec![]) {
                return false;
            }
        }
        true
    }
}
