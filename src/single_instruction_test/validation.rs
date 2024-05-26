use crate::{fat_pointer::FatPointer, State};
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
        self.current_frame.stack.is_valid()
            && self
                .previous_frames
                .iter()
                .all(|(_, frame)| frame.stack.is_valid())
            && (0..16).all(|i| {
                is_valid_tagged_value((
                    self.registers[i as usize],
                    self.register_pointer_flags & (1 << i) != 0,
                ))
            })
    }
}
