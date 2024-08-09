use crate::instruction_handlers::HeapInterface;
use std::ops::{Index, Range};
use std::{iter, mem};
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
struct HeapPage(Box<[u8; HEAP_PAGE_SIZE]>);

impl Default for HeapPage {
    fn default() -> Self {
        let boxed_slice: Box<[u8]> = vec![0_u8; HEAP_PAGE_SIZE].into();
        Self(boxed_slice.try_into().unwrap()) // FIXME: bench `unwrap_unchecked()`?
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Heap {
    pages: Vec<Option<HeapPage>>,
}

impl Heap {
    fn from_bytes(bytes: &[u8], recycled_pages: &mut Vec<HeapPage>) -> Self {
        let pages = bytes
            .chunks(HEAP_PAGE_SIZE)
            .map(|bytes| {
                Some(if let Some(mut recycled_page) = recycled_pages.pop() {
                    recycled_page.0[..bytes.len()].copy_from_slice(bytes);
                    recycled_page.0[bytes.len()..].fill(0);
                    recycled_page
                } else {
                    let mut boxed_slice: Box<[u8]> = vec![0_u8; HEAP_PAGE_SIZE].into();
                    boxed_slice[..bytes.len()].copy_from_slice(bytes);
                    HeapPage(boxed_slice.try_into().unwrap())
                })
            })
            .collect();
        Self { pages }
    }

    fn with_capacity(capacity: usize, recycled_pages: &mut Vec<HeapPage>) -> Self {
        let end_page = capacity.saturating_sub(1) >> 12;
        let page_count = end_page + 1;
        let new_len = recycled_pages.len().saturating_sub(page_count);
        let recycled_pages = recycled_pages.drain(new_len..).map(|mut page| {
            page.0.fill(0);
            Some(page)
        });
        let pages = recycled_pages
            .chain(iter::repeat_with(|| Some(HeapPage::default())))
            .take(page_count)
            .collect();
        Self { pages }
    }

    fn page(&self, idx: usize) -> Option<&HeapPage> {
        self.pages.get(idx)?.as_ref()
    }

    fn get_or_insert_page(
        &mut self,
        idx: usize,
        recycled_pages: &mut Vec<HeapPage>,
    ) -> &mut HeapPage {
        if self.pages.len() <= idx {
            self.pages.resize(idx + 1, None);
        }
        self.pages[idx].get_or_insert_with(|| recycled_pages.pop().unwrap_or_default())
    }

    fn write_u256(&mut self, start_address: u32, value: U256, recycled_pages: &mut Vec<HeapPage>) {
        let mut bytes = [0; 32];
        value.to_big_endian(&mut bytes);

        let offset = start_address as usize;
        let (page_idx, offset_in_page) = (offset >> 12, offset & (HEAP_PAGE_SIZE - 1));
        let len_in_page = 32.min(HEAP_PAGE_SIZE - offset_in_page);
        let page = self.get_or_insert_page(page_idx, recycled_pages);
        page.0[offset_in_page..(offset_in_page + len_in_page)]
            .copy_from_slice(&bytes[..len_in_page]);

        if len_in_page < 32 {
            let page = self.get_or_insert_page(page_idx + 1, recycled_pages);
            page.0[..32 - len_in_page].copy_from_slice(&bytes[len_in_page..]);
        }
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
        if let Some(page) = self.page(page_idx) {
            result[0..len_in_page]
                .copy_from_slice(&page.0[offset_in_page..(offset_in_page + len_in_page)]);
        }

        if len_in_page < range.len() {
            if let Some(page) = self.page(page_idx + 1) {
                result[len_in_page..range.len()]
                    .copy_from_slice(&page.0[..range.len() - len_in_page]);
            }
        }
        U256::from_big_endian(&result)
    }

    fn read_range_big_endian(&self, range: Range<u32>) -> Vec<u8> {
        let offset = range.start as usize;
        let length = (range.end - range.start) as usize;

        let (mut page_idx, mut offset_in_page) = (offset >> 12, offset & (HEAP_PAGE_SIZE - 1));
        let mut result = Vec::with_capacity(length);
        while result.len() < length {
            let len_in_page = (length - result.len()).min(HEAP_PAGE_SIZE - offset_in_page);
            if let Some(page) = self.page(page_idx) {
                result.extend_from_slice(&page.0[offset_in_page..(offset_in_page + len_in_page)]);
            } else {
                result.resize(result.len() + len_in_page, 0);
            }
            page_idx += 1;
            offset_in_page = 0;
        }
        result
    }
}

#[derive(Debug, Clone)]
pub struct Heaps {
    heaps: Vec<Heap>,
    recycled_pages: Vec<HeapPage>,
    bootloader_heap_rollback_info: Vec<(u32, U256)>,
    bootloader_aux_rollback_info: Vec<(u32, U256)>,
}

pub(crate) const CALLDATA_HEAP: HeapId = HeapId(1);
pub const FIRST_HEAP: HeapId = HeapId(2);
pub(crate) const FIRST_AUX_HEAP: HeapId = HeapId(3);

impl Heaps {
    pub(crate) fn new(calldata: &[u8]) -> Self {
        // The first heap can never be used because heap zero
        // means the current heap in precompile calls
        let mut recycled_pages = vec![];
        Self {
            heaps: vec![
                Heap::default(),
                Heap::from_bytes(calldata, &mut recycled_pages),
                Heap::default(),
                Heap::default(),
            ],
            recycled_pages,
            bootloader_heap_rollback_info: vec![],
            bootloader_aux_rollback_info: vec![],
        }
    }

    pub(crate) fn allocate(&mut self) -> HeapId {
        let id = HeapId(self.heaps.len() as u32);
        self.heaps.push(Heap::with_capacity(
            NEW_FRAME_MEMORY_STIPEND as usize,
            &mut self.recycled_pages,
        ));
        id
    }

    pub(crate) fn allocate_with_content(&mut self, content: &[u8]) -> HeapId {
        self.allocate_inner(content)
    }

    fn allocate_inner(&mut self, memory: &[u8]) -> HeapId {
        let id = HeapId(self.heaps.len() as u32);
        self.heaps
            .push(Heap::from_bytes(memory, &mut self.recycled_pages));
        id
    }

    pub(crate) fn deallocate(&mut self, heap: HeapId) {
        let heap = mem::take(&mut self.heaps[heap.0 as usize]);
        self.recycled_pages.extend(heap.pages.into_iter().flatten());
    }

    pub fn write_u256(&mut self, heap: HeapId, start_address: u32, value: U256) {
        if heap == FIRST_HEAP {
            self.bootloader_heap_rollback_info
                .push((start_address, self[heap].read_u256(start_address)));
        } else if heap == FIRST_AUX_HEAP {
            self.bootloader_aux_rollback_info
                .push((start_address, self[heap].read_u256(start_address)));
        }
        self.heaps[heap.0 as usize].write_u256(start_address, value, &mut self.recycled_pages);
    }

    pub(crate) fn snapshot(&self) -> (usize, usize) {
        (
            self.bootloader_heap_rollback_info.len(),
            self.bootloader_aux_rollback_info.len(),
        )
    }

    pub(crate) fn rollback(&mut self, (heap_snap, aux_snap): (usize, usize)) {
        for (address, value) in self.bootloader_heap_rollback_info.drain(heap_snap..).rev() {
            self.heaps[FIRST_HEAP.0 as usize].write_u256(address, value, &mut self.recycled_pages);
        }
        for (address, value) in self.bootloader_aux_rollback_info.drain(aux_snap..).rev() {
            self.heaps[FIRST_AUX_HEAP.0 as usize].write_u256(
                address,
                value,
                &mut self.recycled_pages,
            );
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

// Since we never remove `Heap` entries (even after rollbacks – although we do deallocate heaps in this case),
// we allow additional empty heaps at the end of `Heaps`.
impl PartialEq for Heaps {
    fn eq(&self, other: &Self) -> bool {
        for i in 0..self.heaps.len().max(other.heaps.len()) {
            if self.heaps.get(i).unwrap_or(&Heap::default())
                != other.heaps.get(i).unwrap_or(&Heap::default())
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

    fn repeat_byte(byte: u8) -> U256 {
        U256::from_little_endian(&[byte; 32])
    }

    fn test_heap_write_resizes(recycled_pages: &mut Vec<HeapPage>) {
        let mut heap = Heap::default();
        heap.write_u256(5, 1.into(), recycled_pages);
        assert_eq!(heap.pages.len(), 1);
        assert_eq!(heap.read_u256(5), 1.into());

        // Check writing at a page boundary
        heap.write_u256(
            HEAP_PAGE_SIZE as u32 - 32,
            repeat_byte(0xaa),
            recycled_pages,
        );
        assert_eq!(heap.pages.len(), 1);
        assert_eq!(
            heap.read_u256(HEAP_PAGE_SIZE as u32 - 32),
            repeat_byte(0xaa)
        );

        for offset in (1..=31).rev() {
            heap.write_u256(
                HEAP_PAGE_SIZE as u32 - offset,
                repeat_byte(offset as u8),
                recycled_pages,
            );
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

        heap.write_u256(1 << 20, repeat_byte(0xff), recycled_pages);
        assert_eq!(heap.pages.len(), 257);
        assert_eq!(heap.pages.iter().flatten().count(), 3);
        assert_eq!(heap.read_u256(1 << 20), repeat_byte(0xff));
    }

    #[test]
    fn heap_write_resizes() {
        test_heap_write_resizes(&mut vec![]);
    }

    #[test]
    fn heap_write_resizes_with_recycled_pages() {
        let mut recycled_pages = vec![HeapPage::default(); 10];
        // Fill all pages with 0xff bytes to detect not clearing pages
        for page in &mut recycled_pages {
            page.0.fill(0xff);
        }
        test_heap_write_resizes(&mut recycled_pages);
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
                let data = heap.read_range_big_endian(offset..offset + length);
                assert_eq!(data.len(), length as usize);
                assert!(data.iter().all(|&byte| byte == 0));
            }
        }

        for (i, offset) in offsets.into_iter().enumerate() {
            let bytes: Vec<_> = (i..i + 32).map(|byte| byte as u8).collect();
            heap.write_u256(offset, U256::from_big_endian(&bytes), &mut vec![]);
            for length in 1..=32 {
                let data = heap.read_range_big_endian(offset..offset + length);
                assert_eq!(data, bytes[..length as usize]);
            }
        }
    }

    #[test]
    fn heap_partial_u256_reads() {
        let mut heap = Heap::default();
        let bytes: Vec<_> = (1..=32).collect();
        heap.write_u256(0, U256::from_big_endian(&bytes), &mut vec![]);
        for length in 1..=32 {
            let read = heap.read_u256_partially(0..length);
            // Mask is 0xff...ff00..00, where the number of `0xff` bytes is the number of read bytes
            let mask = U256::MAX << (8 * (32 - length));
            assert_eq!(read, U256::from_big_endian(&bytes) & mask);
        }

        // The same test at the page boundary.
        let offset = HEAP_PAGE_SIZE as u32 - 10;
        heap.write_u256(offset, U256::from_big_endian(&bytes), &mut vec![]);
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
        let heap = Heap::with_capacity(NEW_FRAME_MEMORY_STIPEND as usize, &mut vec![]);
        assert_eq!(heap.pages.len(), 1);
        assert_eq!(*heap.pages[0].as_ref().unwrap().0, [0_u8; 4096]);
    }

    fn test_creating_heap_from_bytes(recycled_pages: &mut Vec<HeapPage>) {
        let bytes: Vec<_> = (0..=u8::MAX).collect();
        let heap = Heap::from_bytes(&bytes, recycled_pages);
        assert_eq!(heap.pages.len(), 1);

        assert_eq!(heap.read_range_big_endian(0..256), bytes);
        for offset in 0..256 - 32 {
            let value = heap.read_u256(offset as u32);
            assert_eq!(value, U256::from_big_endian(&bytes[offset..offset + 32]));
        }

        // Test larger heap with multiple pages.
        let bytes: Vec<_> = (0..HEAP_PAGE_SIZE * 5 / 2).map(|byte| byte as u8).collect();
        let heap = Heap::from_bytes(&bytes, recycled_pages);
        assert_eq!(heap.pages.len(), 3);

        assert_eq!(
            heap.read_range_big_endian(0..HEAP_PAGE_SIZE as u32 * 5 / 2),
            bytes
        );
        for len in [
            1,
            10,
            100,
            HEAP_PAGE_SIZE / 3,
            HEAP_PAGE_SIZE / 2,
            HEAP_PAGE_SIZE,
            2 * HEAP_PAGE_SIZE,
        ] {
            for offset in 0..(HEAP_PAGE_SIZE * 5 / 2 - len) {
                assert_eq!(
                    heap.read_range_big_endian(offset as u32..(offset + len) as u32),
                    bytes[offset..offset + len]
                );
            }
        }

        for offset in 0..HEAP_PAGE_SIZE * 5 / 2 - 32 {
            let value = heap.read_u256(offset as u32);
            assert_eq!(value, U256::from_big_endian(&bytes[offset..offset + 32]));
        }
    }

    #[test]
    fn creating_heap_from_bytes() {
        test_creating_heap_from_bytes(&mut vec![]);
    }

    #[test]
    fn creating_heap_from_bytes_with_recycling() {
        let mut recycled_pages = vec![HeapPage::default(); 10];
        // Fill all pages with 0xff bytes to detect not clearing pages
        for page in &mut recycled_pages {
            page.0.fill(0xff);
        }
        test_creating_heap_from_bytes(&mut recycled_pages);
    }
}
