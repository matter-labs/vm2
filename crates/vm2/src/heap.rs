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

/// Heap page.
#[derive(Debug, Clone, PartialEq)]
struct HeapPage(Box<[u8; HEAP_PAGE_SIZE]>);

impl Default for HeapPage {
    fn default() -> Self {
        let boxed_slice: Box<[u8]> = vec![0_u8; HEAP_PAGE_SIZE].into();
        Self(boxed_slice.try_into().unwrap())
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Heap {
    pages: Vec<Option<HeapPage>>,
    /// Logical byte size counted against [`Heaps::live_logical_bytes`]. This is a purely
    /// host-side bookkeeping quantity (not part of VM-visible state) used to enforce a
    /// per-transaction heap ceiling; it defaults to 0 and is only ever set explicitly via
    /// [`Heaps::set_counted_size`].
    counted_size: u32,
}

// The reference VM treats reads from missing memory pages as reads from an
// all-zero page. Keep write paths strict, but make read-only indexing total so
// panic-produced or otherwise empty fat pointers cannot abort the host.
static EMPTY_HEAP: Heap = Heap {
    pages: Vec::new(),
    counted_size: 0,
};

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
                    if page.0.iter().any(|&byte| byte != 0) {
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
                Some(if let Some(mut page) = pagepool.get_dirty_page() {
                    page.0[..bytes.len()].copy_from_slice(bytes);
                    page.0[bytes.len()..].fill(0);
                    page
                } else {
                    let mut page = HeapPage::default();
                    page.0[..bytes.len()].copy_from_slice(bytes);
                    page
                })
            })
            .collect();
        Self {
            pages,
            counted_size: 0,
        }
    }

    fn recycle(self, pagepool: &mut PagePool) {
        for page in self.pages.into_iter().flatten() {
            pagepool.recycle_page(page);
        }
    }

    pub(crate) fn read_u256(&self, start_address: u32) -> U256 {
        let (page_idx, offset_in_page) = address_to_page_offset(start_address);
        let bytes_in_page = HEAP_PAGE_SIZE - offset_in_page;

        if bytes_in_page >= 32 {
            if let Some(page) = self.page(page_idx) {
                U256::from_big_endian(&page.0[offset_in_page..offset_in_page + 32])
            } else {
                U256::zero()
            }
        } else {
            let mut result = [0u8; 32];
            if let Some(page) = self.page(page_idx) {
                for (res, src) in result.iter_mut().zip(&page.0[offset_in_page..]) {
                    *res = *src;
                }
            }
            if let Some(page) = self.page(page_idx + 1) {
                for (res, src) in result[bytes_in_page..].iter_mut().zip(&*page.0) {
                    *res = *src;
                }
            }
            U256::from_big_endian(&result)
        }
    }

    pub(crate) fn read_u256_partially(&self, range: Range<u32>) -> U256 {
        let (page_idx, offset_in_page) = address_to_page_offset(range.start);
        let length = range.len();
        let bytes_in_page = length.min(HEAP_PAGE_SIZE - offset_in_page);

        let mut result = [0u8; 32];
        if let Some(page) = self.page(page_idx) {
            for (res, src) in result[..bytes_in_page]
                .iter_mut()
                .zip(&page.0[offset_in_page..])
            {
                *res = *src;
            }
        }
        if let Some(page) = self.page(page_idx + 1) {
            for (res, src) in result[bytes_in_page..length].iter_mut().zip(&*page.0) {
                *res = *src;
            }
        }
        U256::from_big_endian(&result)
    }

    pub(crate) fn read_range_big_endian(&self, range: Range<u32>) -> Vec<u8> {
        let length = range.len();

        let (mut page_idx, mut offset_in_page) = address_to_page_offset(range.start);
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

    /// Needed only by tracers
    pub(crate) fn read_byte(&self, address: u32) -> u8 {
        let (page, offset) = address_to_page_offset(address);
        self.page(page).map_or(0, |page| page.0[offset])
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
        let page = self.get_or_insert_page(page_idx, pagepool);

        if bytes_in_page >= 32 {
            value.to_big_endian(&mut page.0[offset_in_page..offset_in_page + 32]);
        } else {
            let mut bytes = [0; 32];
            value.to_big_endian(&mut bytes);
            let mut bytes_iter = bytes.into_iter();

            for (dst, src) in page.0[offset_in_page..].iter_mut().zip(bytes_iter.by_ref()) {
                *dst = src;
            }

            let page = self.get_or_insert_page(page_idx + 1, pagepool);
            for (dst, src) in page.0.iter_mut().zip(bytes_iter) {
                *dst = src;
            }
        }
    }

    fn write_bytes(&mut self, start_address: u32, bytes: &[u8], pagepool: &mut PagePool) {
        let (mut page_idx, mut offset_in_page) = address_to_page_offset(start_address);
        let mut remaining = bytes;
        while !remaining.is_empty() {
            let bytes_in_page = (HEAP_PAGE_SIZE - offset_in_page).min(remaining.len());
            let page = self.get_or_insert_page(page_idx, pagepool);
            page.0[offset_in_page..offset_in_page + bytes_in_page]
                .copy_from_slice(&remaining[..bytes_in_page]);
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
    /// Running sum of `counted_size` over all live heaps, excluding the always-allocated
    /// bootloader heaps (`FIRST`/`FIRST_AUX`, static memory, calldata), whose `counted_size`
    /// is never set and stays 0. Feeds the per-transaction heap ceiling enforced elsewhere.
    live_logical_bytes: u64,
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
            live_logical_bytes: 0,
        }
    }

    /// Pre-reserve capacity for the dynamic heap-pages Vec to avoid Vec
    /// doubling during execution. Each `far_call` allocates a new
    /// `DynamicPageGroup`; large batches push this Vec past 70+ MiB through
    /// the doubling sequence. A one-shot reserve keeps it as a single
    /// allocation.
    pub(crate) fn reserve_dynamic_groups(&mut self, n: usize) {
        let extra = n.saturating_sub(self.dynamic.len());
        if extra > 0 {
            self.dynamic.reserve_exact(extra);
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

        self.live_logical_bytes -= u64::from(self[page].counted_size);

        let heap = decoded_dynamic_slot_mut(&mut self.dynamic, decoded)
            .take()
            .unwrap_or_else(|| panic!("heap page {} is not allocated", page.as_u32()));
        heap.recycle(&mut self.pagepool);
    }

    pub(crate) fn dynamic_len(&self) -> usize {
        self.dynamic.len()
    }

    /// Sets the logical byte size counted against [`Self::live_logical_bytes`] for `page`,
    /// adjusting the running total by the signed delta between the old and new sizes. Safe to
    /// call repeatedly (idempotent for an unchanged `size`); used both for initial accounting
    /// and later growth.
    pub(crate) fn set_counted_size(&mut self, page: HeapId, size: u32) {
        let decoded = DecodedPage::decode(page)
            .unwrap_or_else(|| panic!("heap page {} is not decodable", page.as_u32()));
        let heap = self
            .try_decoded_page_mut(decoded)
            .unwrap_or_else(|| panic!("heap page {} is not allocated", page.as_u32()));
        let old = heap.counted_size;
        heap.counted_size = size;
        self.live_logical_bytes = self.live_logical_bytes - u64::from(old) + u64::from(size);
    }

    /// Running sum of [`set_counted_size`](Self::set_counted_size) sizes over all live heaps.
    pub(crate) fn live_logical_bytes(&self) -> u64 {
        self.live_logical_bytes
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

    pub(crate) fn snapshot(&self) -> (usize, usize, u64) {
        (
            self.bootloader_heap_rollback_info.len(),
            self.bootloader_aux_rollback_info.len(),
            self.live_logical_bytes,
        )
    }

    pub(crate) fn rollback(
        &mut self,
        (heap_snap, aux_snap, live_logical_bytes): (usize, usize, u64),
    ) {
        for (address, value) in self.bootloader_heap_rollback_info.drain(heap_snap..).rev() {
            self.bootloader_heap
                .write_u256(address, value, &mut self.pagepool);
        }

        for (address, value) in self.bootloader_aux_rollback_info.drain(aux_snap..).rev() {
            self.bootloader_aux_heap
                .write_u256(address, value, &mut self.pagepool);
        }

        self.live_logical_bytes = live_logical_bytes;
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

    fn try_decoded_page_mut(&mut self, page: DecodedPage) -> Option<&mut Heap> {
        match page {
            DecodedPage::Static => Some(&mut self.static_memory),
            DecodedPage::BootloaderCalldata => Some(&mut self.bootloader_calldata),
            DecodedPage::BootloaderHeap => Some(&mut self.bootloader_heap),
            DecodedPage::BootloaderAuxHeap => Some(&mut self.bootloader_aux_heap),
            DecodedPage::Dynamic { group, kind } => self
                .dynamic
                .get_mut(group)
                .and_then(|group| group.slot_mut(kind).as_mut()),
        }
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
    fn allocate_page(&mut self) -> HeapPage {
        self.get_dirty_page()
            .map(|mut page| {
                page.0.fill(0);
                page
            })
            .unwrap_or_default()
    }

    fn get_dirty_page(&mut self) -> Option<HeapPage> {
        self.0.pop()
    }

    fn recycle_page(&mut self, page: HeapPage) {
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
            let mut page = HeapPage::default();
            // Fill pages with 0xff bytes to detect not clearing pages
            page.0.fill(0xff);
            pagepool.recycle_page(page);
        }
        pagepool
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
        assert_eq!(snapshot, (1, 1, 0));

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
    fn counter_tracks_set_and_deallocate() {
        let mut heaps = Heaps::new(&[]);
        assert_eq!(heaps.live_logical_bytes(), 0);
        let page = crate::page_ids::heap_page_from_base(crate::page_ids::first_dynamic_base_page());
        let p = heaps.allocate_at(page);
        heaps.set_counted_size(p, 4096);
        assert_eq!(heaps.live_logical_bytes(), 4096);
        heaps.set_counted_size(p, 10_000); // grow
        assert_eq!(heaps.live_logical_bytes(), 10_000);
        heaps.deallocate(p);
        assert_eq!(heaps.live_logical_bytes(), 0);
    }

    #[test]
    fn counter_saved_and_restored_by_snapshot() {
        let mut heaps = Heaps::new(&[]);
        let base_page = crate::page_ids::first_dynamic_base_page();
        let page = crate::page_ids::heap_page_from_base(base_page);
        let other_page =
            crate::page_ids::heap_page_from_base(crate::page_ids::next_page_group(base_page));
        let p = heaps.allocate_at(page);
        heaps.set_counted_size(p, 8192);
        let snap = heaps.snapshot();
        let q = heaps.allocate_at(other_page);
        heaps.set_counted_size(q, 5000);
        assert_eq!(heaps.live_logical_bytes(), 13_192);
        heaps.rollback(snap);
        assert_eq!(heaps.live_logical_bytes(), 8192);
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
