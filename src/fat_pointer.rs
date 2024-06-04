use crate::heap::HeapId;
use u256::U256;
use zkevm_opcode_defs::FatPointerValidationException;

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

    pub fn validate(&self, is_fresh: bool) -> FatPointerValidationException {
        let mut exceptions = FatPointerValidationException::empty();

        // we have 2 invariants:
        // fresh one has `offset` == 0
        if is_fresh && self.offset != 0 {
            exceptions.set(
                FatPointerValidationException::OFFSET_IS_NOT_ZERO_WHEN_EXPECTED,
                true,
            );
        }
        // start + length doesn't overflow
        let (_, of) = self.start.overflowing_add(self.length);
        if of {
            exceptions.set(FatPointerValidationException::DEREF_BEYOND_HEAP_RANGE, true);
        }

        exceptions
    }

    /// We allow to pass empty (offset == length) slices in Far call / Ret
    pub const fn validate_as_slice(&self) -> bool {
        let is_valid_slice = self.offset <= self.length;

        is_valid_slice
    }

    pub(crate) fn empty() -> Self {
        Self {
            offset: 0,
            memory_page: HeapId::from_u32_unchecked(0),
            start: 0,
            length: 0,
        }
    }
}
