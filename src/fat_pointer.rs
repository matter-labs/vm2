use crate::heap::HeapId;
use u256::U256;

#[derive(Debug)]
#[repr(C)]
pub struct FatPointer {
    pub offset: u32,
    pub memory_page: HeapId,
    pub start: u32,
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
    #[cfg(target_endian = "little")]
    pub fn into_u256(self) -> U256 {
        U256::zero() + unsafe { std::mem::transmute::<FatPointer, u128>(self) }
    }
}
