use crate::instruction_handlers::HeapInterface;
use std::{
    collections::HashMap,
    ops::{Index, IndexMut, Range},
    rc::Rc,
};
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

/// Heap page size in bytes.
const HEAP_PAGE_SIZE: usize = 1 << 12;

/// Heap page.
#[derive(Debug, Clone, PartialEq)]
struct HeapPage(Rc<[u8; HEAP_PAGE_SIZE]>);

impl Default for HeapPage {
    fn default() -> Self {
        // FIXME: try reusing pages (w/ thread-local arena tied to execution?)
        let boxed_slice: Box<[u8]> = vec![0_u8; HEAP_PAGE_SIZE].into();
        let boxed_slice: Box<[u8; HEAP_PAGE_SIZE]> = boxed_slice.try_into().unwrap();
        Self(boxed_slice.into())
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Heap {
    // FIXME: try other data structs?
    pages: HashMap<usize, HeapPage>,
}

impl Heap {
    fn from_bytes(bytes: &[u8]) -> Self {
        let pages = bytes
            .chunks(HEAP_PAGE_SIZE)
            .map(|bytes| {
                let boxed_slice: Box<[u8]> = vec![0_u8; HEAP_PAGE_SIZE].into();
                let mut boxed_slice: Box<[u8; HEAP_PAGE_SIZE]> = boxed_slice.try_into().unwrap();
                boxed_slice[..bytes.len()].copy_from_slice(bytes);
                HeapPage(boxed_slice.into())
            })
            .enumerate()
            .collect();
        Self { pages }
    }

    fn with_capacity(capacity: usize) -> Self {
        let end_page = capacity.saturating_sub(1) >> 12;
        let pages = (0..=end_page)
            .map(|idx| (idx, HeapPage::default()))
            .collect();
        Self { pages }
    }
}

impl HeapInterface for Heap {
    fn read_u256(&self, start_address: u32) -> U256 {
        self.read_u256_partially(start_address..start_address + 32)
    }

    fn read_u256_partially(&self, range: Range<u32>) -> U256 {
        let offset = range.start as usize;
        let (page_idx, offset_in_page) = (offset >> 12, offset & (HEAP_PAGE_SIZE - 1));
        let mut result = [0_u8; 32];
        let len_in_page = range.len().min(HEAP_PAGE_SIZE - offset_in_page);
        if let Some(page) = self.pages.get(&page_idx) {
            result[0..len_in_page]
                .copy_from_slice(&page.0[offset_in_page..(offset_in_page + len_in_page)]);
        }

        if len_in_page < range.len() {
            if let Some(page) = self.pages.get(&(page_idx + 1)) {
                result[len_in_page..range.len()]
                    .copy_from_slice(&page.0[..range.len() - len_in_page]);
            }
        }
        U256::from_big_endian(&result)
    }

    fn write_u256(&mut self, start_address: u32, value: U256) {
        let mut bytes = [0; 32];
        value.to_big_endian(&mut bytes);

        let offset = start_address as usize;
        let (page_idx, offset_in_page) = (offset >> 12, offset & (HEAP_PAGE_SIZE - 1));
        let len_in_page = 32.min(HEAP_PAGE_SIZE - offset_in_page);
        let page = self.pages.entry(page_idx).or_default();
        Rc::make_mut(&mut page.0)[offset_in_page..(offset_in_page + len_in_page)]
            .copy_from_slice(&bytes[..len_in_page]);

        if len_in_page < 32 {
            let page = self.pages.entry(page_idx + 1).or_default();
            Rc::make_mut(&mut page.0)[..32 - len_in_page].copy_from_slice(&bytes[len_in_page..]);
        }
    }

    fn read_range(&self, offset: u32, length: u32) -> Vec<u8> {
        let offset = offset as usize;
        let length = length as usize;

        let (mut page_idx, mut offset_in_page) = (offset >> 12, offset & (HEAP_PAGE_SIZE - 1));
        let mut result = Vec::with_capacity(length);
        while result.len() < length {
            let len_in_page = (length - result.len()).min(HEAP_PAGE_SIZE - offset_in_page);
            if let Some(page) = self.pages.get(&page_idx) {
                result.extend_from_slice(&page.0[offset_in_page..(offset_in_page + len_in_page)]);
            } else {
                result.resize(result.len() + len_in_page, 0);
            }
            page_idx += 1;
            offset_in_page = 0;
        }
        result
    }

    fn memset(&mut self, src: &[u8]) {
        *self = Self::from_bytes(src);
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
            Heap::default(),
            Heap::from_bytes(&calldata),
            Heap::default(),
            Heap::default(),
        ])
    }

    pub(crate) fn allocate(&mut self) -> HeapId {
        let id = HeapId(self.0.len() as u32);
        self.0
            .push(Heap::with_capacity(NEW_FRAME_MEMORY_STIPEND as usize));
        id
    }

    pub(crate) fn deallocate(&mut self, heap: HeapId) {
        self.0[heap.0 as usize] = Heap::default();
    }

    /// Creates a heaps snapshot for the root callframe. This uses a fact that all heaps other than `FIRST_HEAP` and `FIRST_AUX_HEAP`
    /// are immutable for this frame.
    pub(crate) fn root_snapshot(&self) -> HeapsSnapshot {
        HeapsSnapshot {
            mutable_heaps: vec![
                (FIRST_HEAP, self[FIRST_HEAP].clone()),
                (FIRST_AUX_HEAP, self[FIRST_AUX_HEAP].clone()),
            ],
            len: self.0.len(),
        }
    }

    /// Restores the heaps from the provided snapshot.
    pub(crate) fn restore_from_snapshot(&mut self, snapshot: HeapsSnapshot) {
        self.0.truncate(snapshot.len);
        for (heap_id, heap) in snapshot.mutable_heaps {
            self[heap_id] = heap;
        }
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
            if self.0.get(i).unwrap_or(&Heap::default())
                != other.0.get(i).unwrap_or(&Heap::default())
            {
                return false;
            }
        }
        true
    }
}

/// Snapshot of [`Heaps`].
#[derive(Debug)]
pub(crate) struct HeapsSnapshot {
    mutable_heaps: Vec<(HeapId, Heap)>,
    len: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use u256::U256;

    fn repeat_byte(byte: u8) -> U256 {
        U256::from_little_endian(&[byte; 32])
    }

    #[test]
    fn heap_write_resizes() {
        let mut heap = Heap::default();
        heap.write_u256(5, 1.into());
        assert_eq!(heap.pages.len(), 1);
        assert_eq!(heap.read_u256(5), 1.into());

        // Check writing at a page boundary
        heap.write_u256(HEAP_PAGE_SIZE as u32 - 32, repeat_byte(0xaa));
        assert_eq!(heap.pages.len(), 1);
        assert_eq!(
            heap.read_u256(HEAP_PAGE_SIZE as u32 - 32),
            repeat_byte(0xaa)
        );

        for offset in (1..=31).rev() {
            heap.write_u256(HEAP_PAGE_SIZE as u32 - offset, repeat_byte(offset as u8));
            assert_eq!(heap.pages.len(), 2);
            assert_eq!(
                heap.read_u256(HEAP_PAGE_SIZE as u32 - offset),
                repeat_byte(offset as u8)
            );
        }

        // check reading at a page boundary from a missing page
        for offset in 0..32 {
            assert_eq!(heap.read_u256((1 << 20) - offset), 0.into());
        }

        heap.write_u256(1 << 20, repeat_byte(0xff));
        assert_eq!(heap.pages.len(), 3);
        assert_eq!(heap.read_u256(1 << 20), repeat_byte(0xff));
    }

    #[test]
    fn reading_heap_range() {
        let mut heap = Heap::default();
        let offsets = [
            0_u32,
            10,
            HEAP_PAGE_SIZE as u32 - 10,
            HEAP_PAGE_SIZE as u32 + 10,
            (1 << 20) - 10,
            1 << 20,
            (1 << 20) + 10,
        ];
        for offset in offsets {
            for length in [0, 1, 10, 31, 32, 1_024, 32_768] {
                let data = heap.read_range(offset, length);
                assert_eq!(data.len(), length as usize);
                assert!(data.iter().all(|&byte| byte == 0));
            }
        }

        for (i, offset) in offsets.into_iter().enumerate() {
            let bytes: Vec<_> = (i..i + 32).map(|byte| byte as u8).collect();
            heap.write_u256(offset, U256::from_big_endian(&bytes));
            for length in 1..=32 {
                let data = heap.read_range(offset, length);
                assert_eq!(data, bytes[..length as usize]);
            }
        }
    }

    #[test]
    fn heap_partial_u256_reads() {
        let mut heap = Heap::default();
        let bytes: Vec<_> = (1..=32).collect();
        heap.write_u256(0, U256::from_big_endian(&bytes));
        for length in 1..=32 {
            let read = heap.read_u256_partially(0..length);
            // Mask is 0xff...ff00..00, where the number of `0xff` bytes is the number of read bytes
            let mask = U256::MAX << (8 * (32 - length));
            assert_eq!(read, U256::from_big_endian(&bytes) & mask);
        }

        // The same test at the page boundary.
        let offset = HEAP_PAGE_SIZE as u32 - 10;
        heap.write_u256(offset, U256::from_big_endian(&bytes));
        for length in 1..=32 {
            let read = heap.read_u256_partially(offset..offset + length);
            let mask = U256::MAX << (8 * (32 - length));
            assert_eq!(read, U256::from_big_endian(&bytes) & mask);
        }
    }

    #[test]
    fn heap_read_out_of_bounds() {
        let heap = Heap::default();
        assert_eq!(heap.read_u256(5), 0.into());
    }

    #[test]
    fn default_new_heap_does_not_allocate_many_pages() {
        let heap = Heap::with_capacity(NEW_FRAME_MEMORY_STIPEND as usize);
        assert_eq!(heap.pages.len(), 1);
        assert_eq!(*heap.pages[&0].0, [0_u8; 4096]);
    }
}
