use crate::bitset::Bitset;
use std::alloc::{alloc, alloc_zeroed, Layout};
use u256::U256;

#[derive(PartialEq, Debug)]
pub struct Stack {
    /// set of slots that may be interpreted as [crate::fat_pointer::FatPointer].
    pointer_flags: Bitset,
    dirty_areas: u64,
    slots: [U256; 1 << 16],
}

const NUMBER_OF_DIRTY_AREAS: usize = 64;
const DIRTY_AREA_SIZE: usize = (1 << 16) / NUMBER_OF_DIRTY_AREAS;

impl Stack {
    pub(crate) fn new() -> Box<Self> {
        unsafe { Box::from_raw(alloc_zeroed(Layout::new::<Stack>()).cast::<Stack>()) }
    }

    pub(crate) fn get(&self, slot: u16) -> U256 {
        self.slots[slot as usize]
    }

    pub(crate) fn set(&mut self, slot: u16, value: U256) {
        let written_area = slot as usize / DIRTY_AREA_SIZE;
        self.dirty_areas |= 1 << written_area;

        self.slots[slot as usize] = value;
    }

    fn zero(&mut self) {
        for i in 0..NUMBER_OF_DIRTY_AREAS {
            if self.dirty_areas & (1 << i) != 0 {
                for slot in self.slots[i * DIRTY_AREA_SIZE..(i + 1) * DIRTY_AREA_SIZE].iter_mut() {
                    *slot = U256::zero();
                }
            }
        }

        self.dirty_areas = 0;
        self.pointer_flags = Default::default();
    }

    pub(crate) fn get_pointer_flag(&self, slot: u16) -> bool {
        self.pointer_flags.get(slot)
    }

    pub(crate) fn set_pointer_flag(&mut self, slot: u16) {
        self.pointer_flags.set(slot);
    }

    pub(crate) fn clear_pointer_flag(&mut self, slot: u16) {
        self.pointer_flags.clear(slot);
    }

    pub(crate) fn snapshot(&self) -> StackSnapshot {
        let dirty_prefix_end = NUMBER_OF_DIRTY_AREAS - self.dirty_areas.leading_zeros() as usize;

        StackSnapshot {
            pointer_flags: self.pointer_flags.clone(),
            dirty_areas: self.dirty_areas,
            slots: self.slots[..DIRTY_AREA_SIZE * dirty_prefix_end].into(),
        }
    }

    pub(crate) fn rollback(&mut self, snapshot: StackSnapshot) {
        let StackSnapshot {
            pointer_flags,
            dirty_areas,
            slots,
        } = snapshot;

        self.zero();

        self.pointer_flags = pointer_flags;
        self.dirty_areas = dirty_areas;
        for (me, snapshot) in self.slots.iter_mut().zip(slots.iter()) {
            *me = *snapshot;
        }
    }
}

pub(crate) struct StackSnapshot {
    pointer_flags: Bitset,
    dirty_areas: u64,
    slots: Box<[U256]>,
}

impl Clone for Box<Stack> {
    fn clone(&self) -> Self {
        unsafe {
            let allocation = alloc(Layout::for_value(&**self)).cast();
            std::ptr::copy_nonoverlapping(&**self, allocation, 1);
            Box::from_raw(allocation)
        }
    }
}

#[derive(Default)]
pub struct StackPool {
    stacks: Vec<Box<Stack>>,
}

impl StackPool {
    pub fn get(&mut self) -> Box<Stack> {
        self.stacks
            .pop()
            .map(|mut s| {
                s.zero();
                s
            })
            .unwrap_or_else(Stack::new)
    }

    pub fn recycle(&mut self, stack: Box<Stack>) {
        self.stacks.push(stack);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The code produced by derive(Clone) overflows the stack in debug mode.
    #[test]
    fn clone_does_not_segfault() {
        let stack = Stack::new();
        let _ = stack.clone();
    }
}
