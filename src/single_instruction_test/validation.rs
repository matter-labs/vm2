use crate::{callframe::Callframe, fat_pointer::FatPointer, State};
use u256::U256;

pub(crate) fn is_valid_tagged_value((value, is_pointer): (U256, bool)) -> bool {
    if is_pointer {
        let pointer = FatPointer::from(value);
        pointer.start.checked_add(pointer.length).is_some()
    } else {
        true
    }
}

impl State {
    pub(crate) fn is_valid(&self) -> bool {
        self.current_frame.is_valid()
            && self
                .previous_frames
                .iter()
                .all(|(_, frame)| frame.is_valid())
            && (0..16).all(|i| {
                is_valid_tagged_value((
                    self.registers[i as usize],
                    self.register_pointer_flags & (1 << i) != 0,
                ))
            })
    }
}

impl Callframe {
    pub(crate) fn is_valid(&self) -> bool {
        self.stack.is_valid()
            && self.calldata_heap != self.heap
            && self.calldata_heap != self.aux_heap
    }
}
