use super::mock_array::MockRead;
use crate::instruction_handlers::HeapInterface;
use arbitrary::Arbitrary;
use std::ops::{Index, IndexMut};
use u256::U256;

#[derive(Debug, Clone)]
pub struct Heap {
    pub(crate) read: MockRead<u32, [u8; 32]>,
    pub(crate) write: Option<(u32, U256)>,
}

impl HeapInterface for Heap {
    fn read_u256(&self, start_address: u32) -> U256 {
        assert!(self.write.is_none());
        U256::from_little_endian(self.read.get(start_address))
    }

    fn read_u256_partially(&self, range: std::ops::Range<u32>) -> U256 {
        assert!(self.write.is_none());
        let mut result = *self.read.get(range.start);
        for byte in &mut result[0..range.len()] {
            *byte = 0;
        }
        U256::from_little_endian(&result)
    }

    fn write_u256(&mut self, start_address: u32, value: U256) {
        assert!(self.write.is_none());
        self.write = Some((start_address, value));
    }

    fn read_range_big_endian(&self, _: std::ops::Range<u32>) -> Vec<u8> {
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

#[derive(Debug, Clone, Arbitrary)]
pub struct Heaps {
    pub(crate) read: MockRead<HeapId, Heap>,
}

pub(crate) const CALLDATA_HEAP: HeapId = HeapId(1);
pub const FIRST_HEAP: HeapId = HeapId(2);
pub(crate) const FIRST_AUX_HEAP: HeapId = HeapId(3);

impl Heaps {
    pub(crate) fn new(_: Vec<u8>) -> Self {
        unimplemented!("Should use arbitrary heap, not fresh heap in testing.")
    }

    pub(crate) fn allocate(&mut self) -> HeapId {
        HeapId(0)
    }

    pub(crate) fn deallocate(&mut self, _: HeapId) {}
}

impl Index<HeapId> for Heaps {
    type Output = Heap;

    fn index(&self, index: HeapId) -> &Self::Output {
        self.read.get(index)
    }
}

impl IndexMut<HeapId> for Heaps {
    fn index_mut(&mut self, index: HeapId) -> &mut Self::Output {
        self.read.get_mut(index)
    }
}

impl PartialEq for Heaps {
    fn eq(&self, _: &Self) -> bool {
        false
    }
}

#[derive(Copy, Clone, PartialEq, Debug, Arbitrary)]
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
