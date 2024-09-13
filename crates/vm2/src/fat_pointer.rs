use primitive_types::U256;
use zksync_vm2_interface::HeapId;

/// Fat pointer to a heap location.
#[derive(Debug)]
#[repr(C)]
pub struct FatPointer {
    /// Additional pointer offset inside the `start..(start + length)` range.
    pub offset: u32,
    /// ID of the heap page this points to.
    pub memory_page: HeapId,
    /// 0-based index of the pointer start byte at the `memory` page.
    pub start: u32,
    /// Length of the pointed slice in bytes.
    pub length: u32,
}

#[cfg(target_endian = "little")]
impl From<&mut U256> for &mut FatPointer {
    fn from(value: &mut U256) -> Self {
        unsafe { &mut *(value as *mut U256).cast() }
    }
}

#[cfg(target_endian = "little")]
impl From<U256> for FatPointer {
    fn from(value: U256) -> Self {
        unsafe {
            let ptr: *const FatPointer = (&value as *const U256).cast();
            ptr.read()
        }
    }
}

impl FatPointer {
    /// Converts this pointer into a `U256` word.
    #[cfg(target_endian = "little")]
    pub fn into_u256(self) -> U256 {
        U256::zero() + unsafe { std::mem::transmute::<FatPointer, u128>(self) }
    }
}
