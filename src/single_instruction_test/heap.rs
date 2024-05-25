use super::mock_array::MockRead;
use arbitrary::Arbitrary;
use std::ops::{Index, IndexMut};

//#[derive(Debug, Clone)]
type Heap = Vec<u8>;

#[derive(Debug, Clone)]
pub struct Heaps {
    read: MockRead<HeapId, Heap>,
}

impl<'a> Arbitrary<'a> for Heaps {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        Ok(Self {
            read: MockRead::new(vec![u.arbitrary()?; 1]),
        })
    }
}

pub(crate) const CALLDATA_HEAP: HeapId = HeapId(1);
pub const FIRST_HEAP: HeapId = HeapId(2);
pub(crate) const FIRST_AUX_HEAP: HeapId = HeapId(3);

impl Heaps {
    pub(crate) fn new(_: Vec<u8>) -> Self {
        unimplemented!("Should use arbitrary heap, not fresh heap in testing.")
    }

    pub(crate) fn allocate(&mut self) -> HeapId {
        todo!()
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
