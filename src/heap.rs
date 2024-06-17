use crate::instruction_handlers::HeapInterface;
use std::ops::{Index, IndexMut, Range};
use u256::U256;
use zkevm_opcode_defs::system_params::NEW_FRAME_MEMORY_STIPEND;

#[derive(Copy, Clone, PartialEq, Debug)]
pub struct HeapId(u32);

impl HeapId {
    /// Only for dealing with external data structures, never use internally.
    pub fn from_u32_unchecked(value: u32) -> Self {
        Self(value)
    }

    pub fn to_u32(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Heap(Vec<u8>);

impl Heap {
    pub fn reserve(&mut self, additional: usize) {
        self.0.reserve_exact(additional);
    }
}

impl HeapInterface for Heap {
    fn read_u256(&self, start_address: u32) -> U256 {
        self.read_u256_partially(start_address..start_address + 32)
    }
    fn read_u256_partially(&self, range: Range<u32>) -> U256 {
        let end = (range.end as usize).min(self.0.len());

        let mut bytes = [0; 32];
        for (i, j) in (range.start as usize..end).enumerate() {
            bytes[i] = self.0[j];
        }
        U256::from_big_endian(&bytes)
    }
    fn write_u256(&mut self, start_address: u32, value: U256) {
        let end = (start_address + 32) as usize;
        if end > self.0.len() {
            self.0.resize(end, 0);
        }

        let mut bytes = [0; 32];
        value.to_big_endian(&mut bytes);
        self.0[start_address as usize..end].copy_from_slice(&bytes);
    }
    fn read_range_big_endian(&self, range: Range<u32>) -> Vec<u8> {
        let end = (range.end as usize).min(self.0.len());
        let mut result = vec![0; range.len()];
        for (i, j) in (range.start as usize..end).enumerate() {
            result[i] = self.0[j];
        }
        result
    }
    fn memset(&mut self, src: &[U256]) {
        for (i, word) in src.iter().enumerate() {
            self.write_u256((i * 32) as u32, *word);
        }
    }
}

#[derive(Debug, Clone)]
pub struct Heaps(Vec<Heap>);

pub(crate) const CALLDATA_HEAP: HeapId = HeapId(1);
pub const FIRST_HEAP: HeapId = HeapId(2);
pub(crate) const FIRST_AUX_HEAP: HeapId = HeapId(3);

impl Heaps {
    pub(crate) fn new(calldata: Vec<u8>) -> Self {
        // The first heap can never be used because heap zero
        // means the current heap in precompile calls
        Self(vec![
            Heap(vec![]),
            Heap(calldata),
            Heap(vec![]),
            Heap(vec![]),
        ])
    }

    pub(crate) fn allocate(&mut self) -> HeapId {
        let id = HeapId(self.0.len() as u32);
        self.0
            .push(Heap(vec![0; NEW_FRAME_MEMORY_STIPEND as usize]));
        id
    }

    pub(crate) fn deallocate(&mut self, heap: HeapId) {
        self.0[heap.0 as usize].0 = vec![];
    }
}

impl Index<HeapId> for Heaps {
    type Output = Heap;

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
            if self.0.get(i).unwrap_or(&Heap(vec![])) != other.0.get(i).unwrap_or(&Heap(vec![])) {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heap_write_resizes() {
        let mut heap = Heap(vec![]);
        heap.write_u256(5, 1.into());
        assert_eq!(heap.read_u256(5), 1.into());
    }

    #[test]
    fn heap_read_out_of_bounds() {
        let heap = Heap(vec![]);
        assert_eq!(heap.read_u256(5), 0.into());
    }
}
