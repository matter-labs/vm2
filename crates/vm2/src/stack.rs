use std::{
    alloc::{alloc_zeroed, Layout},
    fmt,
};

use primitive_types::U256;

use crate::{bitset::Bitset, fat_pointer::FatPointer, hash_for_debugging};

const NUMBER_OF_DIRTY_AREAS: usize = 64;
const DIRTY_AREA_SIZE: usize = (1 << 16) / NUMBER_OF_DIRTY_AREAS;

/// A contiguous block of `DIRTY_AREA_SIZE` stack slots, allocated on demand.
type SlotChunk = Box<[U256; DIRTY_AREA_SIZE]>;

/// Allocate a zeroed slot chunk without materializing it on the caller's stack
/// frame first (a `[U256; DIRTY_AREA_SIZE]` temporary is 32 KiB). `U256`'s
/// all-zero bit pattern is the integer zero, so `alloc_zeroed` is sound.
#[allow(clippy::cast_ptr_alignment)] // aligned per the array layout
fn zeroed_chunk() -> SlotChunk {
    unsafe { Box::from_raw(alloc_zeroed(Layout::new::<[U256; DIRTY_AREA_SIZE]>()).cast()) }
}

/// VM stack.
///
/// The slots are stored as [`NUMBER_OF_DIRTY_AREAS`] chunks that are allocated
/// lazily on first write, so a frame that touches only a handful of slots pays
/// for one 32 KiB chunk rather than the full 2 MiB. An absent chunk reads as
/// all-zero, exactly like the dense zero-initialized array it replaces — this
/// is purely a memory-layout change with no observable difference. Keeping the
/// backing sparse bounds the memory held by deep call stacks (each live frame
/// keeps its own `Stack`, and `StackPool` retains them for reuse).
#[derive(Clone)]
pub(crate) struct Stack {
    /// set of slots that may be interpreted as [`FatPointer`].
    pointer_flags: Bitset,
    dirty_areas: u64,
    slots: [Option<SlotChunk>; NUMBER_OF_DIRTY_AREAS],
}

impl Stack {
    #[allow(clippy::cast_ptr_alignment)] // aligned per `Stack` layout
    pub(crate) fn new() -> Box<Self> {
        // A zeroed `Stack` is valid: `Bitset` is all-zero, `dirty_areas` is 0,
        // and `Option<Box<_>>` uses the null-pointer niche, so all chunks are `None`.
        unsafe { Box::from_raw(alloc_zeroed(Layout::new::<Self>()).cast()) }
    }

    #[inline(always)]
    pub(crate) fn get(&self, slot: u16) -> U256 {
        let area = slot as usize / DIRTY_AREA_SIZE;
        match &self.slots[area] {
            Some(chunk) => chunk[slot as usize % DIRTY_AREA_SIZE],
            None => U256::zero(),
        }
    }

    #[inline(always)]
    pub(crate) fn set(&mut self, slot: u16, value: U256) {
        let area = slot as usize / DIRTY_AREA_SIZE;
        // Mark the area dirty on every write (including zero writes), matching
        // the previous dense implementation so `dirty_areas` evolves identically.
        self.dirty_areas |= 1 << area;
        let chunk = self.slots[area].get_or_insert_with(zeroed_chunk);
        chunk[slot as usize % DIRTY_AREA_SIZE] = value;
    }

    fn zero(&mut self) {
        // Dropping a dirty chunk returns it to all-zero (absent reads as zero).
        // Only dirty areas can hold a chunk, so this clears everything.
        for i in 0..NUMBER_OF_DIRTY_AREAS {
            if self.dirty_areas & (1 << i) != 0 {
                self.slots[i] = None;
            }
        }

        self.dirty_areas = 0;
        self.pointer_flags = Bitset::default();
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

        // Materialize the same dense prefix the dense implementation stored:
        // areas `0..dirty_prefix_end`, with absent chunks contributing zeros.
        let mut slots = vec![U256::zero(); DIRTY_AREA_SIZE * dirty_prefix_end];
        for i in 0..dirty_prefix_end {
            if let Some(chunk) = &self.slots[i] {
                slots[i * DIRTY_AREA_SIZE..(i + 1) * DIRTY_AREA_SIZE].copy_from_slice(&chunk[..]);
            }
        }

        StackSnapshot {
            pointer_flags: self.pointer_flags.clone(),
            dirty_areas: self.dirty_areas,
            slots: slots.into_boxed_slice(),
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
        // Every dirty area lies within the saved prefix (`dirty_prefix_end` is
        // the highest dirty area + 1), so restore a chunk for each from the
        // prefix. Non-dirty areas stay absent and read as zero, exactly as they
        // were in the snapshot.
        for i in 0..NUMBER_OF_DIRTY_AREAS {
            if dirty_areas & (1 << i) != 0 {
                let mut chunk = zeroed_chunk();
                chunk.copy_from_slice(&slots[i * DIRTY_AREA_SIZE..(i + 1) * DIRTY_AREA_SIZE]);
                self.slots[i] = Some(chunk);
            }
        }
    }
}

impl PartialEq for Stack {
    fn eq(&self, other: &Self) -> bool {
        if self.dirty_areas != other.dirty_areas || self.pointer_flags != other.pointer_flags {
            return false;
        }
        // Compare slot values, treating an absent chunk as all-zero. This
        // reproduces the derived comparison over the previous dense array.
        (0..NUMBER_OF_DIRTY_AREAS).all(|area| match (&self.slots[area], &other.slots[area]) {
            (Some(a), Some(b)) => a == b,
            (Some(chunk), None) | (None, Some(chunk)) => chunk.iter().all(U256::is_zero),
            (None, None) => true,
        })
    }
}

pub(crate) struct StackSnapshot {
    pointer_flags: Bitset,
    dirty_areas: u64,
    slots: Box<[U256]>,
}

#[derive(Debug, Default)]
pub(crate) struct StackPool {
    stacks: Vec<Box<Stack>>,
}

impl StackPool {
    pub(crate) fn get(&mut self) -> Box<Stack> {
        self.stacks.pop().map_or_else(Stack::new, |mut s| {
            s.zero();
            s
        })
    }

    pub(crate) fn recycle(&mut self, stack: Box<Stack>) {
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
        const DEBUGGED_SLOTS: u16 = 256;

        let slots = (0..DEBUGGED_SLOTS).map(|idx| (self.pointer_flags.get(idx), self.get(idx)));
        let materialized: Vec<U256> = (0..DEBUGGED_SLOTS).map(|idx| self.get(idx)).collect();
        formatter
            .debug_struct("Stack")
            .field("start", &StackStart(slots))
            .field(
                "pointer_flags.hash",
                &hash_for_debugging(&self.pointer_flags),
            )
            .field("slots.hash", &hash_for_debugging(&materialized))
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
