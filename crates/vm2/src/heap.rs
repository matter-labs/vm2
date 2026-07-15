use std::{
    fmt,
    ops::{Index, Range},
};

use primitive_types::U256;
use zkevm_opcode_defs::{NEW_MEMORY_PAGES_PER_FAR_CALL, STARTING_BASE_PAGE};
use zksync_vm2_interface::HeapId;

use crate::page_ids::{
    bootloader_aux_heap_page, bootloader_calldata_page, bootloader_heap_page, static_memory_page,
};

/// Heap page size in bytes.
const HEAP_PAGE_SIZE: usize = 1 << 12;

/// Sub-page allocation granularity in bytes. A [`HeapPage`] only allocates the
/// chunks that are actually written; untouched chunks read as zero, exactly as
/// a dense zero page would. This bounds the memory of the "write one byte per
/// page and keep the page alive" growth pattern: a boundary write now costs one
/// 256-byte chunk instead of a full 4 KiB page (~16x less), while the fixed
/// per-page index stays small (16 * 8 = 128 bytes).
const HEAP_CHUNK_SIZE: usize = 256;
const CHUNKS_PER_PAGE: usize = HEAP_PAGE_SIZE / HEAP_CHUNK_SIZE;

/// Heap page stored as lazily-allocated fixed-size chunks. An absent chunk is
/// semantically an all-zero chunk; reads and equality treat it as such, so the
/// observable behavior is identical to a dense zero-initialized page.
#[derive(Debug, Clone)]
struct HeapPage {
    chunks: [Option<Box<[u8; HEAP_CHUNK_SIZE]>>; CHUNKS_PER_PAGE],
}

impl HeapPage {
    fn new() -> Self {
        Self {
            chunks: std::array::from_fn(|_| None),
        }
    }

    /// Drop every chunk, returning the page to the all-zero state. Cheap: frees
    /// the chunk allocations rather than zeroing 4 KiB in place.
    fn clear(&mut self) {
        for chunk in &mut self.chunks {
            *chunk = None;
        }
    }

    fn is_all_zero(&self) -> bool {
        self.chunks
            .iter()
            .flatten()
            .all(|chunk| chunk.iter().all(|&byte| byte == 0))
    }

    fn byte(&self, offset: usize) -> u8 {
        match &self.chunks[offset / HEAP_CHUNK_SIZE] {
            Some(chunk) => chunk[offset % HEAP_CHUNK_SIZE],
            None => 0,
        }
    }

    /// Copy `dst.len()` bytes starting at `offset` (which must satisfy
    /// `offset + dst.len() <= HEAP_PAGE_SIZE`) into `dst`, filling regions
    /// backed by absent chunks with zero. Spans crossing chunk boundaries are
    /// handled internally.
    fn read_into(&self, offset: usize, dst: &mut [u8]) {
        let mut pos = 0;
        while pos < dst.len() {
            let abs = offset + pos;
            let chunk_idx = abs / HEAP_CHUNK_SIZE;
            let in_chunk = abs % HEAP_CHUNK_SIZE;
            let n = (HEAP_CHUNK_SIZE - in_chunk).min(dst.len() - pos);
            match &self.chunks[chunk_idx] {
                Some(chunk) => dst[pos..pos + n].copy_from_slice(&chunk[in_chunk..in_chunk + n]),
                None => dst[pos..pos + n].fill(0),
            }
            pos += n;
        }
    }

    /// Write `src` starting at `offset` (which must satisfy
    /// `offset + src.len() <= HEAP_PAGE_SIZE`), allocating any touched chunks.
    fn write_from(&mut self, offset: usize, src: &[u8]) {
        let mut pos = 0;
        while pos < src.len() {
            let abs = offset + pos;
            let chunk_idx = abs / HEAP_CHUNK_SIZE;
            let in_chunk = abs % HEAP_CHUNK_SIZE;
            let n = (HEAP_CHUNK_SIZE - in_chunk).min(src.len() - pos);
            let chunk =
                self.chunks[chunk_idx].get_or_insert_with(|| Box::new([0u8; HEAP_CHUNK_SIZE]));
            chunk[in_chunk..in_chunk + n].copy_from_slice(&src[pos..pos + n]);
            pos += n;
        }
    }
}

// Absent and present-but-zero chunks are equivalent, so equality compares the
// effective bytes rather than the chunk-presence structure.
impl PartialEq for HeapPage {
    fn eq(&self, other: &Self) -> bool {
        (0..CHUNKS_PER_PAGE).all(|idx| match (&self.chunks[idx], &other.chunks[idx]) {
            (Some(a), Some(b)) => a == b,
            (Some(chunk), None) | (None, Some(chunk)) => chunk.iter().all(|&byte| byte == 0),
            (None, None) => true,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Heap {
    pages: Vec<Option<HeapPage>>,
}

// The reference VM treats reads from missing memory pages as reads from an
// all-zero page. Keep write paths strict, but make read-only indexing total so
// panic-produced or otherwise empty fat pointers cannot abort the host.
static EMPTY_HEAP: Heap = Heap { pages: Vec::new() };

// We never remove `HeapPage`s (even after rollbacks – although we do zero all added pages in this case),
// we allow additional pages to be present if they are zeroed.
impl PartialEq for Heap {
    fn eq(&self, other: &Self) -> bool {
        for i in 0..self.pages.len().max(other.pages.len()) {
            let this_page = self.pages.get(i).and_then(Option::as_ref);
            let other_page = other.pages.get(i).and_then(Option::as_ref);
            match (this_page, other_page) {
                (Some(this_page), Some(other_page)) => {
                    if this_page != other_page {
                        return false;
                    }
                }
                (Some(page), None) | (None, Some(page)) => {
                    if !page.is_all_zero() {
                        return false;
                    }
                }
                (None, None) => { /* do nothing */ }
            }
        }
        true
    }
}

impl Heap {
    fn from_bytes(bytes: &[u8], pagepool: &mut PagePool) -> Self {
        let pages = bytes
            .chunks(HEAP_PAGE_SIZE)
            .map(|bytes| {
                let mut page = pagepool.allocate_page();
                page.write_from(0, bytes);
                Some(page)
            })
            .collect();
        Self { pages }
    }

    fn recycle(self, pagepool: &mut PagePool) {
        for page in self.pages.into_iter().flatten() {
            pagepool.recycle_page(page);
        }
    }

    pub(crate) fn read_u256(&self, start_address: u32) -> U256 {
        let (page_idx, offset_in_page) = address_to_page_offset(start_address);
        let bytes_in_page = HEAP_PAGE_SIZE - offset_in_page;

        let mut result = [0u8; 32];
        if bytes_in_page >= 32 {
            if let Some(page) = self.page(page_idx) {
                page.read_into(offset_in_page, &mut result);
            }
        } else {
            if let Some(page) = self.page(page_idx) {
                page.read_into(offset_in_page, &mut result[..bytes_in_page]);
            }
            if let Some(page) = self.page(page_idx + 1) {
                page.read_into(0, &mut result[bytes_in_page..]);
            }
        }
        U256::from_big_endian(&result)
    }

    pub(crate) fn read_u256_partially(&self, range: Range<u32>) -> U256 {
        let (page_idx, offset_in_page) = address_to_page_offset(range.start);
        let length = range.len();
        let bytes_in_page = length.min(HEAP_PAGE_SIZE - offset_in_page);

        let mut result = [0u8; 32];
        if let Some(page) = self.page(page_idx) {
            page.read_into(offset_in_page, &mut result[..bytes_in_page]);
        }
        if let Some(page) = self.page(page_idx + 1) {
            page.read_into(0, &mut result[bytes_in_page..length]);
        }
        U256::from_big_endian(&result)
    }

    pub(crate) fn read_range_big_endian(&self, range: Range<u32>) -> Vec<u8> {
        let length = range.len();

        let (mut page_idx, mut offset_in_page) = address_to_page_offset(range.start);
        let mut result = Vec::with_capacity(length);
        while result.len() < length {
            let len_in_page = (length - result.len()).min(HEAP_PAGE_SIZE - offset_in_page);
            let start = result.len();
            result.resize(start + len_in_page, 0);
            if let Some(page) = self.page(page_idx) {
                page.read_into(offset_in_page, &mut result[start..start + len_in_page]);
            }
            page_idx += 1;
            offset_in_page = 0;
        }
        result
    }

    /// Needed only by tracers
    pub(crate) fn read_byte(&self, address: u32) -> u8 {
        let (page, offset) = address_to_page_offset(address);
        self.page(page).map_or(0, |page| page.byte(offset))
    }

    fn page(&self, idx: usize) -> Option<&HeapPage> {
        self.pages.get(idx)?.as_ref()
    }

    fn get_or_insert_page(&mut self, idx: usize, pagepool: &mut PagePool) -> &mut HeapPage {
        if self.pages.len() <= idx {
            self.pages.resize(idx + 1, None);
        }
        self.pages[idx].get_or_insert_with(|| pagepool.allocate_page())
    }

    fn write_u256(&mut self, start_address: u32, value: U256, pagepool: &mut PagePool) {
        let (page_idx, offset_in_page) = address_to_page_offset(start_address);
        let bytes_in_page = HEAP_PAGE_SIZE - offset_in_page;

        let mut bytes = [0u8; 32];
        value.to_big_endian(&mut bytes);

        if bytes_in_page >= 32 {
            self.get_or_insert_page(page_idx, pagepool)
                .write_from(offset_in_page, &bytes);
        } else {
            self.get_or_insert_page(page_idx, pagepool)
                .write_from(offset_in_page, &bytes[..bytes_in_page]);
            self.get_or_insert_page(page_idx + 1, pagepool)
                .write_from(0, &bytes[bytes_in_page..]);
        }
    }

    fn write_bytes(&mut self, start_address: u32, bytes: &[u8], pagepool: &mut PagePool) {
        let (mut page_idx, mut offset_in_page) = address_to_page_offset(start_address);
        let mut remaining = bytes;
        while !remaining.is_empty() {
            let bytes_in_page = (HEAP_PAGE_SIZE - offset_in_page).min(remaining.len());
            self.get_or_insert_page(page_idx, pagepool)
                .write_from(offset_in_page, &remaining[..bytes_in_page]);
            remaining = &remaining[bytes_in_page..];
            page_idx += 1;
            offset_in_page = 0;
        }
    }
}

#[inline(always)]
fn address_to_page_offset(address: u32) -> (usize, usize) {
    let offset = address as usize;
    (offset >> 12, offset & (HEAP_PAGE_SIZE - 1))
}

// TODO: With all the additions, this file should be split into several under `heap/` folder.
// For now I'm keeping it here, since the PR diff is already big and splitting would make the
// diff more obscure.
#[derive(Debug, Clone, Default, PartialEq)]
struct DynamicPageGroup {
    code: Option<Heap>,
    heap: Option<Heap>,
    aux: Option<Heap>,
}

impl DynamicPageGroup {
    fn is_empty(&self) -> bool {
        self.code.is_none() && self.heap.is_none() && self.aux.is_none()
    }

    fn recycle(self, pagepool: &mut PagePool) {
        for heap in [self.code, self.heap, self.aux].into_iter().flatten() {
            heap.recycle(pagepool);
        }
    }

    fn slot(&self, kind: DynamicPageKind) -> Option<&Heap> {
        match kind {
            DynamicPageKind::Code => self.code.as_ref(),
            DynamicPageKind::Heap => self.heap.as_ref(),
            DynamicPageKind::Aux => self.aux.as_ref(),
        }
    }

    fn slot_mut(&mut self, kind: DynamicPageKind) -> &mut Option<Heap> {
        match kind {
            DynamicPageKind::Code => &mut self.code,
            DynamicPageKind::Heap => &mut self.heap,
            DynamicPageKind::Aux => &mut self.aux,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum DynamicPageKind {
    Code,
    Heap,
    Aux,
}

#[derive(Debug, Clone, Copy)]
enum DecodedPage {
    Static,
    BootloaderCalldata,
    BootloaderHeap,
    BootloaderAuxHeap,
    Dynamic { group: usize, kind: DynamicPageKind },
}

impl DecodedPage {
    fn decode(page: HeapId) -> Option<Self> {
        if page == static_memory_page() {
            return Some(Self::Static);
        }
        if page == bootloader_calldata_page() {
            return Some(Self::BootloaderCalldata);
        }
        if page == bootloader_heap_page() {
            return Some(Self::BootloaderHeap);
        }
        if page == bootloader_aux_heap_page() {
            return Some(Self::BootloaderAuxHeap);
        }

        let raw = page.as_u32();
        if raw < STARTING_BASE_PAGE {
            return None;
        }

        let rel = raw - STARTING_BASE_PAGE;
        let group = usize::try_from(rel / NEW_MEMORY_PAGES_PER_FAR_CALL).unwrap();
        match rel % NEW_MEMORY_PAGES_PER_FAR_CALL {
            0 => Some(Self::Dynamic {
                group,
                kind: DynamicPageKind::Code,
            }),
            2 => Some(Self::Dynamic {
                group,
                kind: DynamicPageKind::Heap,
            }),
            3 => Some(Self::Dynamic {
                group,
                kind: DynamicPageKind::Aux,
            }),
            _ => None,
        }
    }

    const fn is_always_allocated(self) -> bool {
        matches!(
            self,
            Self::Static
                | Self::BootloaderCalldata
                | Self::BootloaderHeap
                | Self::BootloaderAuxHeap
        )
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Heaps {
    static_memory: Heap,
    bootloader_calldata: Heap,
    bootloader_heap: Heap,
    bootloader_aux_heap: Heap,
    dynamic: Vec<DynamicPageGroup>,
    pagepool: PagePool,
    bootloader_heap_rollback_info: Vec<(u32, U256)>,
    bootloader_aux_rollback_info: Vec<(u32, U256)>,
}

impl Heaps {
    pub(crate) fn new(calldata: &[u8]) -> Self {
        let mut pagepool = PagePool::default();

        Self {
            static_memory: Heap::from_bytes(&[], &mut pagepool),
            bootloader_calldata: Heap::from_bytes(calldata, &mut pagepool),
            bootloader_heap: Heap::from_bytes(&[], &mut pagepool),
            bootloader_aux_heap: Heap::from_bytes(&[], &mut pagepool),
            dynamic: Vec::new(),
            pagepool,
            bootloader_heap_rollback_info: vec![],
            bootloader_aux_rollback_info: vec![],
        }
    }

    pub(crate) fn allocate_at(&mut self, page: HeapId) -> HeapId {
        self.allocate_with_content_at(page, &[])
    }

    pub(crate) fn allocate_with_content_at(&mut self, page: HeapId, memory: &[u8]) -> HeapId {
        let decoded = DecodedPage::decode(page)
            .unwrap_or_else(|| panic!("heap page {} is not decodable", page.as_u32()));

        assert!(
            !decoded.is_always_allocated(),
            "heap page {} is already allocated",
            page.as_u32()
        );

        let slot = decoded_dynamic_slot_mut(&mut self.dynamic, decoded);
        // `decoded_page_mut_for_write` populates dynamic slots lazily, so in principle a prior
        // write could materialize this slot. In production, `allocate_at` is only called from
        // far-call setup on the heap/aux slots of a freshly assigned base group, which no
        // prior code path writes to. If this fires, that invariant has been broken.
        assert!(
            slot.is_none(),
            "heap page {} is already allocated",
            page.as_u32()
        );
        *slot = Some(Heap::from_bytes(memory, &mut self.pagepool));
        page
    }

    #[cfg(test)]
    pub(crate) fn contains(&self, page: HeapId) -> bool {
        DecodedPage::decode(page).is_some_and(|decoded| {
            decoded.is_always_allocated() || self.try_decoded_page(decoded).is_some()
        })
    }

    // The three panics in this function flag VM-internal bookkeeping bugs, not
    // reachable conditions: the VM only deallocates pages it previously allocated via
    // `allocate_at` (frame pop in `pop_frame`, snapshot rollback in `State::rollback`, and
    // returndata-heap reclaim at tx commit in `reclaim_bootloader_returndata_heaps`). The
    // reference `zk_evm` has no deallocation path of its own, so there is no behaviour to
    // diverge from here — these panics remain as fail-fast diagnostics for our own bookkeeping.
    pub(crate) fn deallocate(&mut self, page: HeapId) {
        let decoded = DecodedPage::decode(page)
            .unwrap_or_else(|| panic!("heap page {} is not decodable", page.as_u32()));

        assert!(
            !decoded.is_always_allocated(),
            "heap page {} must remain allocated",
            page.as_u32()
        );

        let heap = decoded_dynamic_slot_mut(&mut self.dynamic, decoded)
            .take()
            .unwrap_or_else(|| panic!("heap page {} is not allocated", page.as_u32()));
        heap.recycle(&mut self.pagepool);
    }

    pub(crate) fn dynamic_len(&self) -> usize {
        self.dynamic.len()
    }

    pub(crate) fn write_u256(&mut self, page: HeapId, start_address: u32, value: U256) {
        self.record_bootloader_word_rollback(page, start_address);
        // Current callers pass `HeapId`s from VM-controlled sources: store opcodes (current
        // frame heap/aux), static memory, and kernel-gated precompile output. The
        // `StateInterface::write_heap_u256` tracer entry can in principle pass an arbitrary
        // `HeapId`. The reference `zk_evm` would silently materialize any page number; we
        // keep the panic as a tripwire against any new code path.
        let decoded = DecodedPage::decode(page)
            .unwrap_or_else(|| panic!("heap page {} is not decodable", page.as_u32()));
        let (heap, pagepool) = self.decoded_page_mut_for_write(decoded);
        heap.write_u256(start_address, value, pagepool);
    }

    pub(crate) fn write_bytes(&mut self, page: HeapId, start_address: u32, bytes: &[u8]) {
        self.record_bootloader_range_rollback(page, start_address, bytes.len());
        // Same tripwire rationale as `write_u256`: the only production callers go through
        // `materialize_decommit_page` (a fresh code page on far call, or the current frame
        // heap from the `Decommit` opcode), both supplying decodable `HeapId`s. An
        // undecodable page here indicates a new code path that should be reviewed before
        // merging.
        let decoded = DecodedPage::decode(page)
            .unwrap_or_else(|| panic!("heap page {} is not decodable", page.as_u32()));
        let (heap, pagepool) = self.decoded_page_mut_for_write(decoded);
        heap.write_bytes(start_address, bytes, pagepool);
    }

    pub(crate) fn snapshot(&self) -> (usize, usize) {
        (
            self.bootloader_heap_rollback_info.len(),
            self.bootloader_aux_rollback_info.len(),
        )
    }

    pub(crate) fn rollback(&mut self, (heap_snap, aux_snap): (usize, usize)) {
        for (address, value) in self.bootloader_heap_rollback_info.drain(heap_snap..).rev() {
            self.bootloader_heap
                .write_u256(address, value, &mut self.pagepool);
        }

        for (address, value) in self.bootloader_aux_rollback_info.drain(aux_snap..).rev() {
            self.bootloader_aux_heap
                .write_u256(address, value, &mut self.pagepool);
        }
    }

    pub(crate) fn truncate_dynamic_to(&mut self, len: usize) {
        assert!(
            len <= self.dynamic.len(),
            "cannot restore dynamic heap length {} from current length {}",
            len,
            self.dynamic.len()
        );

        for group in self.dynamic.drain(len..) {
            group.recycle(&mut self.pagepool);
        }
    }

    pub(crate) fn delete_history(&mut self) {
        self.bootloader_heap_rollback_info.clear();
        self.bootloader_aux_rollback_info.clear();
    }

    fn record_bootloader_word_rollback(&mut self, page: HeapId, start_address: u32) {
        if page == HeapId::FIRST {
            let prev_value = self[page].read_u256(start_address);
            self.bootloader_heap_rollback_info
                .push((start_address, prev_value));
        } else if page == HeapId::FIRST_AUX {
            let prev_value = self[page].read_u256(start_address);
            self.bootloader_aux_rollback_info
                .push((start_address, prev_value));
        }
    }

    fn record_bootloader_range_rollback(&mut self, page: HeapId, start_address: u32, len: usize) {
        if len == 0 {
            return;
        }

        let start = usize::try_from(start_address).expect("heap write address overflow");
        let end = start.checked_add(len).expect("heap write range overflow");
        let first_word = start / 32 * 32;
        let last_word = (end - 1) / 32 * 32;

        for address in (first_word..=last_word).step_by(32) {
            self.record_bootloader_word_rollback(
                page,
                u32::try_from(address).expect("heap write address overflow"),
            );
        }
    }

    fn try_decoded_page(&self, page: DecodedPage) -> Option<&Heap> {
        match page {
            DecodedPage::Static => Some(&self.static_memory),
            DecodedPage::BootloaderCalldata => Some(&self.bootloader_calldata),
            DecodedPage::BootloaderHeap => Some(&self.bootloader_heap),
            DecodedPage::BootloaderAuxHeap => Some(&self.bootloader_aux_heap),
            DecodedPage::Dynamic { group, kind } => {
                self.dynamic.get(group).and_then(|group| group.slot(kind))
            }
        }
    }

    fn decoded_page(&self, page: DecodedPage) -> &Heap {
        self.try_decoded_page(page).unwrap_or(&EMPTY_HEAP)
    }

    fn decoded_page_mut_for_write(&mut self, page: DecodedPage) -> (&mut Heap, &mut PagePool) {
        let Self {
            static_memory,
            bootloader_calldata,
            bootloader_heap,
            bootloader_aux_heap,
            dynamic,
            pagepool,
            ..
        } = self;

        // The reference memory model materializes a page before writing to it.
        // vm2 keeps the stricter page-id classification, but decodable dynamic
        // pages are valid write targets and should be allocated lazily.
        let heap = match page {
            DecodedPage::Static => static_memory,
            DecodedPage::BootloaderCalldata => bootloader_calldata,
            DecodedPage::BootloaderHeap => bootloader_heap,
            DecodedPage::BootloaderAuxHeap => bootloader_aux_heap,
            DecodedPage::Dynamic { group, kind } => {
                dynamic_slot_mut(dynamic, group, kind).get_or_insert_with(Heap::default)
            }
        };
        (heap, pagepool)
    }
}

impl Index<HeapId> for Heaps {
    type Output = Heap;

    fn index(&self, index: HeapId) -> &Self::Output {
        match DecodedPage::decode(index) {
            Some(decoded) => self.decoded_page(decoded),
            None => &EMPTY_HEAP,
        }
    }
}

impl PartialEq for Heaps {
    fn eq(&self, other: &Self) -> bool {
        if self.static_memory != other.static_memory
            || self.bootloader_calldata != other.bootloader_calldata
            || self.bootloader_heap != other.bootloader_heap
            || self.bootloader_aux_heap != other.bootloader_aux_heap
        {
            return false;
        }

        for idx in 0..self.dynamic.len().max(other.dynamic.len()) {
            match (self.dynamic.get(idx), other.dynamic.get(idx)) {
                (Some(this_group), Some(other_group)) => {
                    if this_group != other_group {
                        return false;
                    }
                }
                (Some(group), None) | (None, Some(group)) => {
                    if !group.is_empty() {
                        return false;
                    }
                }
                (None, None) => {}
            }
        }
        true
    }
}

fn dynamic_slot_mut(
    dynamic: &mut Vec<DynamicPageGroup>,
    group: usize,
    kind: DynamicPageKind,
) -> &mut Option<Heap> {
    if dynamic.len() <= group {
        dynamic.resize_with(group + 1, DynamicPageGroup::default);
    }
    dynamic[group].slot_mut(kind)
}

fn decoded_dynamic_slot_mut(
    dynamic: &mut Vec<DynamicPageGroup>,
    page: DecodedPage,
) -> &mut Option<Heap> {
    let DecodedPage::Dynamic { group, kind } = page else {
        panic!("decoded page {page:?} is not dynamic");
    };
    dynamic_slot_mut(dynamic, group, kind)
}

#[derive(Default, Clone)]
struct PagePool(Vec<HeapPage>);

impl fmt::Debug for PagePool {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PagePool")
            .field("len", &self.0.len())
            .finish_non_exhaustive()
    }
}

impl PagePool {
    /// Returns a cleared page. Pages are cleared on recycle (see
    /// [`Self::recycle_page`]), so a pooled page is already all-zero.
    fn allocate_page(&mut self) -> HeapPage {
        self.0.pop().unwrap_or_else(HeapPage::new)
    }

    /// Recycle a page for reuse. The page's chunks are dropped immediately so
    /// that pooled pages never retain freed heap memory, and so that the next
    /// `allocate_page` hands back an all-zero page.
    fn recycle_page(&mut self, mut page: HeapPage) {
        page.clear();
        self.0.push(page);
    }
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;

    fn repeat_byte(byte: u8) -> U256 {
        U256::from_little_endian(&[byte; 32])
    }

    fn test_heap_write_resizes(recycled_pages: &mut PagePool) {
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
        test_heap_write_resizes(&mut PagePool::default());
    }

    #[test]
    fn heap_write_resizes_with_recycled_pages() {
        test_heap_write_resizes(&mut populated_pagepool());
    }

    fn populated_pagepool() -> PagePool {
        let mut pagepool = PagePool::default();
        for _ in 0..10 {
            let mut page = HeapPage::new();
            // Fill pages with 0xff bytes to detect not clearing pages
            page.write_from(0, &[0xff; HEAP_PAGE_SIZE]);
            pagepool.recycle_page(page);
        }
        pagepool
    }

    // --- Differential fuzz vs a dense oracle -------------------------------
    // The chunked HeapPage is not covered by the `single_instruction_test`
    // divergence harness (that feature substitutes a mock heap), so these
    // tests fuzz the real implementation against a dense byte-vector oracle,
    // biased toward page/chunk boundaries where the split logic lives.

    struct XorShift(u64);
    impl XorShift {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
        fn value(&mut self) -> U256 {
            U256([self.next(), self.next(), self.next(), self.next()])
        }
    }

    // Heap span covering many pages and chunks; U256 writes stay in-bounds.
    const SPAN: usize = 8 * HEAP_PAGE_SIZE; // 8 pages
    const MAX_ADDR: usize = SPAN - 32;

    // Addresses biased to land on and around page (4096) and chunk (256)
    // boundaries, which is exactly where read_into / write_from split.
    fn boundary_biased_addr(rng: &mut XorShift) -> u32 {
        let base = match rng.next() % 5 {
            0 => {
                usize::try_from(rng.next() % (SPAN / HEAP_PAGE_SIZE) as u64).unwrap()
                    * HEAP_PAGE_SIZE
            } // page start
            1 => {
                usize::try_from(rng.next() % (SPAN / HEAP_CHUNK_SIZE) as u64).unwrap()
                    * HEAP_CHUNK_SIZE
            } // chunk start
            _ => usize::try_from(rng.next() % SPAN as u64).unwrap(),
        };
        // Nudge by -16..=+16 to straddle the boundary (saturating, no signed casts).
        let delta = usize::try_from(rng.next() % 33).unwrap();
        let addr = (base + delta).saturating_sub(16).min(MAX_ADDR);
        u32::try_from(addr).unwrap()
    }

    #[test]
    fn differential_random_write_read_matches_dense_oracle() {
        let mut rng = XorShift(0x9e37_79b9_7f4a_7c15);
        let mut heap = Heap::default();
        let mut pool = populated_pagepool();
        let mut oracle = vec![0u8; SPAN];

        for _ in 0..20_000 {
            let addr = boundary_biased_addr(&mut rng);
            let value = if rng.next().is_multiple_of(8) {
                U256::zero() // zero writes must behave like the dense page
            } else {
                rng.value()
            };
            heap.write_u256(addr, value, &mut pool);
            let mut be = [0u8; 32];
            value.to_big_endian(&mut be);
            oracle[addr as usize..addr as usize + 32].copy_from_slice(&be);
        }

        // read_u256 at every boundary-adjacent address.
        for _ in 0..20_000 {
            let addr = boundary_biased_addr(&mut rng);
            let expected = U256::from_big_endian(&oracle[addr as usize..addr as usize + 32]);
            assert_eq!(heap.read_u256(addr), expected, "read_u256 @ {addr}");
        }

        // read_byte across the whole span, incl. never-written (absent) bytes.
        for (addr, expected) in oracle.iter().enumerate() {
            assert_eq!(
                heap.read_byte(u32::try_from(addr).unwrap()),
                *expected,
                "read_byte @ {addr}"
            );
        }

        // read_range_big_endian across random cross-page/cross-chunk ranges.
        for _ in 0..2_000 {
            let start = usize::try_from(rng.next() % SPAN as u64).unwrap();
            let len = usize::try_from(rng.next() % 1024)
                .unwrap()
                .min(SPAN - start);
            let range = u32::try_from(start).unwrap()..u32::try_from(start + len).unwrap();
            let got = heap.read_range_big_endian(range);
            assert_eq!(
                got,
                &oracle[start..start + len],
                "read_range @ {start}+{len}"
            );
        }
    }

    #[test]
    fn from_bytes_matches_dense_oracle() {
        let mut rng = XorShift(0x0123_4567_89ab_cdef);
        // Non-page-multiple length exercises a partial final page/chunk.
        let len = 5 * HEAP_PAGE_SIZE + 137;
        let mut bytes = vec![0u8; len];
        for b in &mut bytes {
            *b = u8::try_from(rng.next() & 0xff).unwrap();
        }
        let mut pool = populated_pagepool();
        let heap = Heap::from_bytes(&bytes, &mut pool);

        for addr in (0..len.saturating_sub(32)).step_by(97) {
            let expected = U256::from_big_endian(&bytes[addr..addr + 32]);
            assert_eq!(
                heap.read_u256(u32::try_from(addr).unwrap()),
                expected,
                "from_bytes @ {addr}"
            );
        }
        // Reading past the initialized content returns zero.
        assert_eq!(heap.read_byte(u32::try_from(len + 500).unwrap()), 0);
    }

    #[test]
    fn pagepool_recycle_hands_back_zeroed_pages() {
        // Dirty a heap, drop it back into the pool, then confirm a new heap
        // built from that pool reads all-zero (no leaked bytes).
        let mut pool = PagePool::default();
        let mut dirty = Heap::default();
        for i in 0..4 {
            dirty.write_u256(i * HEAP_PAGE_SIZE as u32 + 64, repeat_byte(0xcd), &mut pool);
        }
        dirty.recycle(&mut pool);

        let mut fresh = Heap::default();
        // Force allocation of pages that will be drawn from the pool.
        fresh.write_u256(64, 0.into(), &mut pool);
        for addr in 0..HEAP_PAGE_SIZE {
            assert_eq!(fresh.read_byte(addr as u32), 0, "recycled leak @ {addr}");
        }
    }

    #[test]
    fn eq_independent_of_chunk_presence() {
        let mut pool = PagePool::default();
        let mut a = Heap::default();
        let mut b = Heap::default();

        a.write_u256(100, repeat_byte(0x11), &mut pool);
        a.write_u256(5000, repeat_byte(0x22), &mut pool);
        // `a` allocates a chunk at offset 800 then zeroes it (present-but-zero).
        a.write_u256(800, repeat_byte(0x33), &mut pool);
        a.write_u256(800, 0.into(), &mut pool);

        b.write_u256(100, repeat_byte(0x11), &mut pool);
        b.write_u256(5000, repeat_byte(0x22), &mut pool);
        // `b` never touches offset 800 (absent chunk) — must still equal `a`.

        assert_eq!(a, b, "present-zero chunk must equal absent chunk");

        b.write_u256(100, repeat_byte(0x99), &mut pool);
        assert_ne!(a, b, "differing content must be unequal");
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
            heap.write_u256(
                offset,
                U256::from_big_endian(&bytes),
                &mut PagePool::default(),
            );
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
        heap.write_u256(0, U256::from_big_endian(&bytes), &mut PagePool::default());
        for length in 1..=32 {
            let read = heap.read_u256_partially(0..length);
            // Mask is 0xff...ff00..00, where the number of `0xff` bytes is the number of read bytes
            let mask = U256::MAX << (8 * (32 - length));
            assert_eq!(read, U256::from_big_endian(&bytes) & mask);
        }

        // The same test at the page boundary.
        let offset = HEAP_PAGE_SIZE as u32 - 10;
        heap.write_u256(
            offset,
            U256::from_big_endian(&bytes),
            &mut PagePool::default(),
        );
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

    fn test_creating_heap_from_bytes(recycled_pages: &mut PagePool) {
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
        test_creating_heap_from_bytes(&mut PagePool::default());
    }

    #[test]
    fn creating_heap_from_bytes_with_recycling() {
        test_creating_heap_from_bytes(&mut populated_pagepool());
    }

    #[test]
    fn rolling_back_heaps() {
        let mut heaps = Heaps::new(b"test");
        let written_value = U256::from(123_456_789) << 224; // writes bytes 0..4
        heaps.write_u256(HeapId::FIRST, 0, written_value);
        assert_eq!(heaps[HeapId::FIRST].read_u256(0), written_value);
        heaps.write_u256(HeapId::FIRST_AUX, 0, 42.into());
        assert_eq!(heaps[HeapId::FIRST_AUX].read_u256(0), 42.into());

        let snapshot = heaps.snapshot();
        assert_eq!(snapshot, (1, 1));

        heaps.write_u256(HeapId::FIRST, 7, U256::MAX);
        assert_eq!(
            heaps[HeapId::FIRST].read_u256(0),
            written_value + (U256::MAX >> 56)
        );
        heaps.write_u256(HeapId::FIRST_AUX, 16, U256::MAX);
        assert_eq!(heaps[HeapId::FIRST_AUX].read_u256(16), U256::MAX);

        heaps.rollback(snapshot);
        assert_eq!(heaps[HeapId::FIRST].read_u256(0), written_value);
        assert_eq!(heaps[HeapId::FIRST_AUX].read_u256(0), 42.into());
        assert_eq!(heaps.bootloader_heap_rollback_info.len(), 1);
        assert_eq!(heaps.bootloader_aux_rollback_info.len(), 1);
    }

    #[test]
    fn rolling_back_bootloader_range_writes() {
        let mut heaps = Heaps::new(&[]);
        let first_word = repeat_byte(0x11);
        let second_word = repeat_byte(0x22);
        heaps.write_u256(HeapId::FIRST, 0, first_word);
        heaps.write_u256(HeapId::FIRST, 32, second_word);

        let snapshot = heaps.snapshot();

        heaps.write_bytes(HeapId::FIRST, 8, &[0xaa; 40]);
        assert_ne!(heaps[HeapId::FIRST].read_u256(0), first_word);
        assert_ne!(heaps[HeapId::FIRST].read_u256(32), second_word);

        heaps.rollback(snapshot);

        assert_eq!(heaps[HeapId::FIRST].read_u256(0), first_word);
        assert_eq!(heaps[HeapId::FIRST].read_u256(32), second_word);
        assert_eq!(heaps.bootloader_heap_rollback_info.len(), 2);
    }

    #[test]
    fn heaps_ignore_trailing_empty_dynamic_groups_in_equality() {
        let mut with_trailing_group = Heaps::new(&[]);
        let empty = Heaps::new(&[]);
        let page = crate::page_ids::heap_page_from_base(crate::page_ids::first_dynamic_base_page());

        with_trailing_group.allocate_at(page);
        with_trailing_group.deallocate(page);

        assert_eq!(with_trailing_group, empty);
        assert_eq!(empty, with_trailing_group);
    }
}
