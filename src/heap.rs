use crate::instruction_handlers::HeapInterface;
use std::ops::{Index, Range};
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
    fn write_u256(&mut self, start_address: u32, value: U256) {
        let end = (start_address + 32) as usize;
        if end > self.0.len() {
            self.0.resize(end, 0);
        }

        value.to_big_endian(&mut self.0[start_address as usize..end]);
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
    fn read_range_big_endian(&self, range: Range<u32>) -> Vec<u8> {
        let end = (range.end as usize).min(self.0.len());
        let mut result = vec![0; range.len()];
        for (i, j) in (range.start as usize..end).enumerate() {
            result[i] = self.0[j];
        }
        result
    }
}

#[derive(Debug, Clone)]
pub struct Heaps {
    heaps: Vec<Heap>,
    bootloader_heap_rollback_info: Vec<(u32, U256)>,
    bootloader_aux_rollback_info: Vec<(u32, U256)>,
}

pub(crate) const CALLDATA_HEAP: HeapId = HeapId(1);
pub const FIRST_HEAP: HeapId = HeapId(2);
pub(crate) const FIRST_AUX_HEAP: HeapId = HeapId(3);

impl Heaps {
    pub(crate) fn new(calldata: Vec<u8>) -> Self {
        // The first heap can never be used because heap zero
        // means the current heap in precompile calls
        Self {
            heaps: vec![Heap(vec![]), Heap(calldata), Heap(vec![]), Heap(vec![])],
            bootloader_heap_rollback_info: vec![],
        }
    }

    pub(crate) fn allocate(&mut self) -> HeapId {
        self.allocate_inner(vec![0; NEW_FRAME_MEMORY_STIPEND as usize])
    }

    pub(crate) fn allocate_with_content(&mut self, content: &[u8]) -> HeapId {
        self.allocate_inner(content.to_vec())
    }

    fn allocate_inner(&mut self, memory: Vec<u8>) -> HeapId {
        let id = HeapId(self.heaps.len() as u32);
        self.heaps.push(Heap(memory));
        id
    }

    pub(crate) fn deallocate(&mut self, heap: HeapId) {
        self.heaps[heap.0 as usize].0 = vec![];
    }

    pub fn write_u256(&mut self, heap: HeapId, start_address: u32, value: U256) {
        if heap == FIRST_HEAP {
            self.bootloader_heap_rollback_info
                .push((start_address, self[heap].read_u256(start_address)));
        } else if heap == FIRST_AUX_HEAP {
            self.bootloader_aux_rollback_info
                .push((start_address, self[heap].read_u256(start_address)));
        }
        self.heaps[heap.0 as usize].write_u256(start_address, value);
    }

    pub(crate) fn snapshot(&self) -> (usize, usize) {
        (
            self.bootloader_heap_rollback_info.len(),
            self.bootloader_aux_rollback_info.len(),
        )
    }

    pub(crate) fn rollback(&mut self, (heap_snap, aux_snap): (usize, usize)) {
        for (address, value) in self.bootloader_heap_rollback_info.drain(heap_snap..).rev() {
            self.heaps[FIRST_HEAP.0 as usize].write_u256(address, value);
        }
        for (address, value) in self.bootloader_aux_rollback_info.drain(aux_snap..).rev() {
            self.heaps[FIRST_AUX_HEAP.0 as usize].write_u256(address, value);
        }
    }

    pub(crate) fn delete_history(&mut self) {
        self.bootloader_heap_rollback_info.clear();
        self.bootloader_aux_rollback_info.clear();
    }
}

impl Index<HeapId> for Heaps {
    type Output = Heap;

    fn index(&self, index: HeapId) -> &Self::Output {
        &self.heaps[index.0 as usize]
    }
}

impl PartialEq for Heaps {
    fn eq(&self, other: &Self) -> bool {
        for i in 0..self.heaps.len().max(other.heaps.len()) {
            if self.heaps.get(i).unwrap_or(&Heap(vec![]))
                != other.heaps.get(i).unwrap_or(&Heap(vec![]))
            {
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
