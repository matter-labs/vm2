use primitive_types::U256;

use crate::{callframe::Callframe, fat_pointer::FatPointer, state::State};

pub(crate) fn is_valid_tagged_value((value, is_pointer): (U256, bool)) -> bool {
    if is_pointer {
        let pointer = FatPointer::from(value);
        pointer.start.checked_add(pointer.length).is_some()
    } else {
        true
    }
}

impl<T, W> State<T, W> {
    pub(crate) fn is_valid(&self) -> bool {
        self.current_frame.is_valid()
            && self.previous_frames.iter().all(Callframe::is_valid)
            && (0_u16..16).all(|i| {
                is_valid_tagged_value((
                    self.registers[usize::from(i)],
                    self.register_pointer_flags & (1 << i) != 0,
                ))
            })
    }
}

impl<T, W> Callframe<T, W> {
    pub(crate) fn is_valid(&self) -> bool {
        self.stack.is_valid()
    }
}
