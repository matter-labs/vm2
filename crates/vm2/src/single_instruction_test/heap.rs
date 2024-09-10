use std::ops::Index;

use arbitrary::Arbitrary;
use primitive_types::U256;
use zksync_vm2_interface::HeapId;

use super::mock_array::MockRead;

#[derive(Debug, Clone)]
pub struct Heap {
    pub(crate) read: MockRead<u32, [u8; 32]>,
    pub(crate) write: Option<(u32, U256)>,
}

impl Heap {
    fn write_u256(&mut self, start_address: u32, value: U256) {
        assert!(self.write.is_none());
        self.write = Some((start_address, value));
    }

    pub(crate) fn read_byte(&self, _: u32) -> u8 {
        unimplemented!()
    }

    pub(crate) fn read_u256(&self, start_address: u32) -> U256 {
        assert!(self.write.is_none());
        U256::from_little_endian(self.read.get(start_address))
    }

    pub(crate) fn read_u256_partially(&self, range: std::ops::Range<u32>) -> U256 {
        assert!(self.write.is_none());
        let mut result = *self.read.get(range.start);
        for byte in &mut result[0..32 - range.len()] {
            *byte = 0;
        }
        U256::from_little_endian(&result)
    }

    pub(crate) fn read_range_big_endian(&self, _: std::ops::Range<u32>) -> Vec<u8> {
        // This is wrong, but this method is only used to get the final return value.
        vec![]
    }
}

impl<'a> Arbitrary<'a> for Heap {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        Ok(Self {
            read: u.arbitrary()?,
            write: None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Heaps {
    heap_id: HeapId,
    pub(crate) read: MockRead<HeapId, Heap>,
}

impl Heaps {
    pub(crate) fn new(_: &[u8]) -> Self {
        unimplemented!("Should use arbitrary heap, not fresh heap in testing.")
    }

    pub(crate) fn allocate(&mut self) -> HeapId {
        self.heap_id
    }

    pub(crate) fn allocate_with_content(&mut self, content: &[u8]) -> HeapId {
        let id = self.allocate();
        self.read
            .get_mut(id)
            .write_u256(0, U256::from_big_endian(content));
        id
    }

    pub(crate) fn deallocate(&mut self, _: HeapId) {}

    pub(crate) fn from_id(
        heap_id: HeapId,
        u: &mut arbitrary::Unstructured<'_>,
    ) -> arbitrary::Result<Heaps> {
        Ok(Heaps {
            heap_id,
            read: u.arbitrary()?,
        })
    }

    pub fn write_u256(&mut self, heap: HeapId, start_address: u32, value: U256) {
        self.read.get_mut(heap).write_u256(start_address, value);
    }

    pub(crate) fn snapshot(&self) -> (usize, usize) {
        unimplemented!()
    }

    pub(crate) fn rollback(&mut self, _: (usize, usize)) {
        unimplemented!()
    }

    pub(crate) fn delete_history(&mut self) {
        unimplemented!()
    }

    pub(crate) fn write_byte(&mut self, _: HeapId, _: u32, _: u8) {
        unimplemented!()
    }
}

impl Index<HeapId> for Heaps {
    type Output = Heap;

    fn index(&self, index: HeapId) -> &Self::Output {
        self.read.get(index)
    }
}

impl PartialEq for Heaps {
    fn eq(&self, _: &Self) -> bool {
        false
    }
}
