use std::ptr;

use primitive_types::U256;
use zksync_vm2_interface::HeapId;

/// Fat pointer to a heap location.
#[derive(Debug)]
#[repr(C)]
pub struct FatPointer {
    /// Additional pointer offset inside the `start..(start + length)` range.
    pub offset: u32,
    /// ID of the heap this points to.
    pub memory_page: HeapId,
    /// 0-based index of the pointer start byte at the `memory` page.
    pub start: u32,
    /// Length of the pointed slice in bytes.
    pub length: u32,
}

// The `FatPointer <-> U256`/`u128` conversions below reinterpret memory, so they rely on the
// exact layout of `FatPointer` (and, for the `&mut U256` cast, on `U256` being a little-endian
// `[u64; 4]`). These assertions fail to compile if that layout ever drifts, turning silent UB
// into a build error. `HeapId` is `#[repr(transparent)]` so `memory_page` is laid out as a `u32`.
const _: () = assert!(std::mem::size_of::<FatPointer>() == 16);
const _: () = assert!(std::mem::size_of::<FatPointer>() == std::mem::size_of::<u128>());
const _: () = assert!(std::mem::size_of::<HeapId>() == std::mem::size_of::<u32>());
// The `&mut U256 -> &mut FatPointer` cast reinterprets `U256` storage in place, so `FatPointer`
// must fit within a `U256` (size) and be no more aligned than it (alignment) to stay in bounds.
const _: () = assert!(std::mem::size_of::<FatPointer>() <= std::mem::size_of::<U256>());
const _: () = assert!(std::mem::align_of::<FatPointer>() <= std::mem::align_of::<U256>());

#[cfg(target_endian = "little")]
impl From<&mut U256> for &mut FatPointer {
    fn from(value: &mut U256) -> Self {
        unsafe { &mut *ptr::from_mut(value).cast() }
    }
}

#[cfg(target_endian = "little")]
impl From<U256> for FatPointer {
    fn from(value: U256) -> Self {
        unsafe { std::mem::transmute(value.low_u128()) }
    }
}

impl FatPointer {
    /// Converts this pointer into a `U256` word.
    #[cfg(target_endian = "little")]
    pub fn into_u256(self) -> U256 {
        U256::zero() + unsafe { std::mem::transmute::<FatPointer, u128>(self) }
    }
}
