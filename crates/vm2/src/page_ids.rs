use zkevm_opcode_defs::{
    BOOTLOADER_AUX_HEAP_PAGE, BOOTLOADER_CALLDATA_PAGE, BOOTLOADER_HEAP_PAGE,
    NEW_MEMORY_PAGES_PER_FAR_CALL, STARTING_BASE_PAGE, STATIC_MEMORY_PAGE,
};
use zksync_vm2_interface::HeapId;

pub(crate) const fn static_memory_page() -> HeapId {
    HeapId::from_u32_unchecked(STATIC_MEMORY_PAGE)
}

pub(crate) const fn bootloader_calldata_page() -> HeapId {
    HeapId::from_u32_unchecked(BOOTLOADER_CALLDATA_PAGE)
}

pub(crate) const fn bootloader_heap_page() -> HeapId {
    HeapId::from_u32_unchecked(BOOTLOADER_HEAP_PAGE)
}

pub(crate) const fn bootloader_aux_heap_page() -> HeapId {
    HeapId::from_u32_unchecked(BOOTLOADER_AUX_HEAP_PAGE)
}

pub(crate) const fn heap_page_from_base(base_page: u32) -> HeapId {
    HeapId::from_u32_unchecked(base_page + 2)
}

pub(crate) const fn aux_heap_page_from_base(base_page: u32) -> HeapId {
    HeapId::from_u32_unchecked(base_page + 3)
}

pub(crate) const fn code_page_from_base(base_page: u32) -> HeapId {
    HeapId::from_u32_unchecked(base_page)
}

pub(crate) const fn base_page_from_heap(heap: HeapId) -> u32 {
    heap.as_u32() - 2
}

pub(crate) const fn first_dynamic_base_page() -> u32 {
    STARTING_BASE_PAGE
}

pub(crate) const fn next_page_group(page: u32) -> u32 {
    page + NEW_MEMORY_PAGES_PER_FAR_CALL
}
