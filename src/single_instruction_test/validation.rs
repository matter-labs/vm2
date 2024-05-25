use crate::fat_pointer::FatPointer;
use u256::U256;

pub(crate) fn is_valid_tagged_value((value, is_pointer): (U256, bool)) -> bool {
    if is_pointer {
        let pointer = FatPointer::from(value);
        pointer.start.checked_add(pointer.length).is_some()
    } else {
        true
    }
}
