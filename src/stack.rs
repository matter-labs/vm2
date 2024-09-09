use std::{
    alloc::{alloc, alloc_zeroed, Layout},
    fmt,
};

use u256::U256;

use crate::{bitset::Bitset, fat_pointer::FatPointer, hash_for_debugging};

#[derive(PartialEq)]
pub struct Stack {
    /// set of slots that may be interpreted as [`FatPointer`].
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

    #[inline(always)]
    pub(crate) fn get(&self, slot: u16) -> U256 {
        self.slots[slot as usize]
    }

    #[inline(always)]
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

    #[inline(always)]
    pub(crate) fn get_pointer_flag(&self, slot: u16) -> bool {
        self.pointer_flags.get(slot)
    }

    #[inline(always)]
    pub(crate) fn set_pointer_flag(&mut self, slot: u16) {
        self.pointer_flags.set(slot);
    }

    #[inline(always)]
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
        self.slots[..slots.len()].copy_from_slice(&slots);
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

// region:Debug implementations

/// Helper wrapper for debugging [`Stack`] / [`StackSnapshot`] contents.
struct StackStart<I>(I);

impl<I: Iterator<Item = (bool, U256)> + Clone> fmt::Debug for StackStart<I> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut list = formatter.debug_list();
        for (is_pointer, slot) in self.0.clone() {
            if is_pointer {
                list.entry(&FatPointer::from(slot));
            } else {
                list.entry(&slot);
            }
        }
        list.finish()
    }
}

impl fmt::Debug for Stack {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        const DEBUGGED_SLOTS: usize = 256;

        let slots = (0_u16..)
            .zip(&self.slots)
            .map(|(idx, slot)| (self.pointer_flags.get(idx), *slot))
            .take(DEBUGGED_SLOTS);
        formatter
            .debug_struct("Stack")
            .field("start", &StackStart(slots))
            .field(
                "pointer_flags.hash",
                &hash_for_debugging(&self.pointer_flags),
            )
            .field("slots.hash", &hash_for_debugging(&self.slots))
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for StackSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        const DEBUGGED_SLOTS: usize = 256;

        let slots = (0_u16..)
            .zip(&self.slots[..])
            .map(|(idx, slot)| (self.pointer_flags.get(idx), *slot))
            .take(DEBUGGED_SLOTS);
        formatter
            .debug_struct("StackSnapshot")
            .field("dirty_areas", &self.dirty_areas)
            .field("start", &StackStart(slots))
            .field(
                "pointer_flags.hash",
                &hash_for_debugging(&self.pointer_flags),
            )
            .field("slots.hash", &hash_for_debugging(&self.slots))
            .finish_non_exhaustive()
    }
}
// endregion

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
