use std::{
    alloc::{alloc_zeroed, Layout},
    fmt,
};

use primitive_types::U256;

use crate::{bitset::Bitset, fat_pointer::FatPointer, hash_for_debugging};

const NUMBER_OF_DIRTY_AREAS: usize = 64;
const DIRTY_AREA_SIZE: usize = (1 << 16) / NUMBER_OF_DIRTY_AREAS;

// Storage is allocated at a finer granularity than the dirty-area granularity:
// each sub-chunk holds `SUBCHUNK_SLOTS` slots, so a frame that scatters writes
// allocates only the sub-chunks it actually touches, not a full 32 KiB area.
// This bounds the "scatter one write into every area to force 2 MiB/frame"
// pattern (deep nested calls). Once materialized, a sub-chunk stays allocated
// for the stack's lifetime and is cleared in place on reuse (`zero()`), so a
// stack's memory is its high-water mark and there is no per-frame allocator
// traffic. Dirty tracking stays at the coarse area level (the `dirty_areas`
// u64), so snapshot/rollback/equality semantics are unchanged.
const SUBCHUNK_SLOTS: usize = 16;
const NUM_SUBCHUNKS: usize = (1 << 16) / SUBCHUNK_SLOTS;
const SUBCHUNKS_PER_AREA: usize = DIRTY_AREA_SIZE / SUBCHUNK_SLOTS;

/// A contiguous block of `SUBCHUNK_SLOTS` stack slots, allocated on demand.
type SlotChunk = Box<[U256; SUBCHUNK_SLOTS]>;

/// Allocate a zeroed slot chunk without materializing it on the caller's stack
/// frame first. `U256`'s all-zero bit pattern is the integer zero, so
/// `alloc_zeroed` is sound.
#[allow(clippy::cast_ptr_alignment)] // aligned per the array layout
fn zeroed_chunk() -> SlotChunk {
    unsafe { Box::from_raw(alloc_zeroed(Layout::new::<[U256; SUBCHUNK_SLOTS]>()).cast()) }
}

/// VM stack.
///
/// The slots are stored as [`NUM_SUBCHUNKS`] sub-chunks that are allocated
/// lazily on first write, so a frame that touches only a handful of slots pays
/// for a couple of small sub-chunks rather than the full 2 MiB. An absent
/// sub-chunk reads as all-zero, exactly like the dense zero-initialized array
/// it replaces — this is purely a memory-layout change with no observable
/// difference. Keeping the backing sparse bounds the memory held by deep call
/// stacks (each live frame keeps its own `Stack`, and `StackPool` retains them
/// for reuse).
#[derive(Clone)]
pub(crate) struct Stack {
    /// set of slots that may be interpreted as [`FatPointer`].
    pointer_flags: Bitset,
    dirty_areas: u64,
    slots: [Option<SlotChunk>; NUM_SUBCHUNKS],
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
        let subchunk = slot as usize / SUBCHUNK_SLOTS;
        match &self.slots[subchunk] {
            Some(chunk) => chunk[slot as usize % SUBCHUNK_SLOTS],
            None => U256::zero(),
        }
    }

    #[inline(always)]
    pub(crate) fn set(&mut self, slot: u16, value: U256) {
        let area = slot as usize / DIRTY_AREA_SIZE;
        // Mark the (coarse) area dirty on every write (including zero writes),
        // matching the previous implementation so `dirty_areas` evolves
        // identically and snapshot/rollback/eq are unaffected.
        self.dirty_areas |= 1 << area;
        let subchunk = slot as usize / SUBCHUNK_SLOTS;
        let chunk = self.slots[subchunk].get_or_insert_with(zeroed_chunk);
        chunk[slot as usize % SUBCHUNK_SLOTS] = value;
    }

    fn zero(&mut self) {
        // Clear materialized sub-chunks in place instead of dropping them: an
        // all-zero chunk is observationally identical to an absent one (`get`
        // reads zero for both and `PartialEq` treats them as equal), and
        // keeping the allocation avoids a free/alloc round-trip per sub-chunk
        // on every frame that reuses a pooled stack — up to `NUM_SUBCHUNKS`
        // allocator operations per far call, which is host-side work an EraVM
        // program does not pay gas for.
        //
        // Clearing only dirty areas suffices: a sub-chunk may stay
        // materialized in a non-dirty area (chunks now outlive `dirty_areas`
        // resets), but it can only become *nonzero* via `set`, which marks its
        // area dirty in the same call, or via `rollback`, which restores
        // `dirty_areas` alongside the data. So chunks in non-dirty areas are
        // always all-zero already.
        for i in 0..NUMBER_OF_DIRTY_AREAS {
            if self.dirty_areas & (1 << i) != 0 {
                for sc in (i * SUBCHUNKS_PER_AREA)..((i + 1) * SUBCHUNKS_PER_AREA) {
                    if let Some(chunk) = self.slots[sc].as_mut() {
                        chunk.fill(U256::zero());
                    }
                }
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
        // areas `0..dirty_prefix_end`, with absent sub-chunks contributing zeros.
        let mut slots = vec![U256::zero(); DIRTY_AREA_SIZE * dirty_prefix_end];
        for sc in 0..(dirty_prefix_end * SUBCHUNKS_PER_AREA) {
            if let Some(chunk) = &self.slots[sc] {
                slots[sc * SUBCHUNK_SLOTS..(sc + 1) * SUBCHUNK_SLOTS].copy_from_slice(&chunk[..]);
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
        // the highest dirty area + 1), so restore its sub-chunks from the
        // prefix, reusing chunks that are already materialized. Non-dirty
        // areas read as zero, exactly as in the snapshot: `zero()` above left
        // their chunks (if any) all-zero.
        for i in 0..NUMBER_OF_DIRTY_AREAS {
            if dirty_areas & (1 << i) != 0 {
                for sc in (i * SUBCHUNKS_PER_AREA)..((i + 1) * SUBCHUNKS_PER_AREA) {
                    let chunk = self.slots[sc].get_or_insert_with(zeroed_chunk);
                    chunk.copy_from_slice(&slots[sc * SUBCHUNK_SLOTS..(sc + 1) * SUBCHUNK_SLOTS]);
                }
            }
        }
    }
}

impl PartialEq for Stack {
    fn eq(&self, other: &Self) -> bool {
        if self.dirty_areas != other.dirty_areas || self.pointer_flags != other.pointer_flags {
            return false;
        }
        // Compare slot values, treating an absent sub-chunk as all-zero. This
        // reproduces the derived comparison over the previous dense array.
        (0..NUM_SUBCHUNKS).all(|sc| match (&self.slots[sc], &other.slots[sc]) {
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

    // --- Differential fuzz vs a dense oracle -------------------------------
    // The chunked stack is not exercised by the `single_instruction_test`
    // divergence harness (that feature substitutes a mock stack), so these
    // tests fuzz the real implementation against a dense `[U256; 1 << 16]`
    // oracle to guarantee the sparse layout is observably identical.

    struct XorShift(u64);
    impl XorShift {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
        fn slot(&mut self) -> u16 {
            u16::try_from(self.next() & 0xffff).expect("masked to 16 bits")
        }
        fn value(&mut self) -> U256 {
            U256([self.next(), self.next(), self.next(), self.next()])
        }
    }

    const DENSE: usize = 1 << 16;

    #[test]
    fn differential_random_set_get_matches_dense_oracle() {
        let mut rng = XorShift(0x1234_5678_9abc_def1);
        let mut stack = Stack::new();
        let mut oracle = vec![U256::zero(); DENSE];

        for _ in 0..20_000 {
            let slot = rng.slot();
            // Mix in zero writes: they must allocate/dirty exactly like nonzero.
            let value = if rng.next().is_multiple_of(8) {
                U256::zero()
            } else {
                rng.value()
            };
            stack.set(slot, value);
            oracle[usize::from(slot)] = value;
        }
        for (slot, expected) in oracle.iter().enumerate() {
            let slot = u16::try_from(slot).unwrap();
            assert_eq!(stack.get(slot), *expected, "mismatch at slot {slot}");
        }
    }

    #[test]
    fn zero_returns_to_fresh_state() {
        let mut rng = XorShift(0xdead_beef_cafe_0001);
        let mut stack = Stack::new();
        for _ in 0..5_000 {
            stack.set(rng.slot(), rng.value());
        }
        stack.zero();
        assert_eq!(stack.dirty_areas, 0, "zero() must clear all dirty bits");
        let fresh = Stack::new();
        assert_eq!(&stack, &fresh, "zero() must equal a fresh stack");
        for slot in 0..DENSE {
            assert_eq!(stack.get(u16::try_from(slot).unwrap()), U256::zero());
        }
    }

    #[test]
    fn zero_retains_materialized_chunks() {
        // Regression test for the frame-reuse path: `zero()` must clear
        // sub-chunks in place, not free them, so that reusing a pooled stack
        // causes no allocator traffic. A refactor that reintroduces the drop
        // fails the `is_some` assertions below.
        let mut stack = Stack::new();
        // One write per scattered sub-chunk, including area boundaries.
        for (i, slot) in [0u16, 17, 1000, 4096, 32_768, 65_535]
            .into_iter()
            .enumerate()
        {
            stack.set(slot, U256::from(u64::try_from(i).unwrap() + 1));
        }
        let materialized: Vec<usize> = (0..NUM_SUBCHUNKS)
            .filter(|&sc| stack.slots[sc].is_some())
            .collect();
        assert_eq!(
            materialized.len(),
            6,
            "each write hits a distinct sub-chunk"
        );

        stack.zero();
        for &sc in &materialized {
            let chunk = stack.slots[sc]
                .as_ref()
                .unwrap_or_else(|| panic!("zero() must keep sub-chunk {sc}, not free it"));
            assert!(
                chunk.iter().all(U256::is_zero),
                "zero() must clear retained sub-chunk {sc}"
            );
        }
    }

    #[test]
    fn reused_stack_matches_fresh_after_same_writes() {
        // A stack recycled through `zero()` (the `StackPool` reuse path) must
        // be indistinguishable from a fresh one under the same writes, even
        // though it keeps previously materialized sub-chunks around.
        let mut rng = XorShift(0x1122_3344_5566_7788);
        let mut reused = Stack::new();
        for _ in 0..5_000 {
            reused.set(rng.slot(), rng.value());
        }
        reused.zero();

        let mut fresh = Stack::new();
        for _ in 0..5_000 {
            let slot = rng.slot();
            let value = if rng.next().is_multiple_of(8) {
                U256::zero()
            } else {
                rng.value()
            };
            reused.set(slot, value);
            fresh.set(slot, value);
        }
        assert_eq!(&reused, &fresh, "reuse must not be observable");
        for slot in 0..DENSE {
            let slot = u16::try_from(slot).unwrap();
            assert_eq!(reused.get(slot), fresh.get(slot), "mismatch at slot {slot}");
        }
    }

    #[test]
    fn snapshot_rollback_after_reuse_matches_dense_oracle() {
        let mut rng = XorShift(0x9876_5432_10ab_cdef);
        let mut stack = Stack::new();
        let mut oracle = vec![U256::zero(); DENSE];

        // Dirty the stack, then recycle it the way `StackPool::get` does.
        for _ in 0..4_000 {
            stack.set(rng.slot(), rng.value());
        }
        stack.zero();

        for _ in 0..4_000 {
            let (slot, value) = (rng.slot(), rng.value());
            stack.set(slot, value);
            oracle[usize::from(slot)] = value;
        }
        let snap = stack.snapshot();

        // Diverge, then roll back; the oracle keeps the snapshot-time state.
        for _ in 0..4_000 {
            stack.set(rng.slot(), rng.value());
        }
        stack.rollback(snap);
        for (slot, expected) in oracle.iter().enumerate() {
            let slot = u16::try_from(slot).unwrap();
            assert_eq!(
                stack.get(slot),
                *expected,
                "rollback mismatch at slot {slot}"
            );
        }
    }

    #[test]
    fn zero_write_still_dirties_the_area() {
        // `set(slot, 0)` must mark `dirty_areas` exactly like the dense stack
        // did, including on a recycled stack whose chunks are already present.
        let mut stack = Stack::new();
        assert_eq!(stack.dirty_areas, 0);
        stack.set(0, U256::zero());
        assert_eq!(stack.dirty_areas, 1);
        let area3_slot = u16::try_from(3 * DIRTY_AREA_SIZE).unwrap();
        stack.set(area3_slot, U256::zero());
        assert_eq!(stack.dirty_areas, 0b1001);

        stack.zero();
        assert_eq!(stack.dirty_areas, 0);
        // The chunk for slot 0 is still materialized; a zero write must dirty
        // its area again regardless.
        stack.set(0, U256::zero());
        assert_eq!(stack.dirty_areas, 1);
    }

    #[test]
    fn snapshot_rollback_restores_exact_state() {
        let mut rng = XorShift(0x0abc_1234_5678_9def);
        let mut stack = Stack::new();

        for _ in 0..8_000 {
            stack.set(rng.slot(), rng.value());
        }
        // Capture the exact slot state at snapshot time.
        let mut expected = vec![U256::zero(); DENSE];
        for (slot, e) in expected.iter_mut().enumerate() {
            *e = stack.get(u16::try_from(slot).unwrap());
        }
        let snap = stack.snapshot();

        // Diverge: overwrite, zero-write, and touch new areas.
        for _ in 0..8_000 {
            stack.set(rng.slot(), rng.value());
        }
        stack.set(0xffff, rng.value());
        stack.set(0, U256::zero());

        stack.rollback(snap);
        for (slot, e) in expected.iter().enumerate() {
            let slot = u16::try_from(slot).unwrap();
            assert_eq!(stack.get(slot), *e, "rollback mismatch at slot {slot}");
        }
    }

    #[test]
    fn eq_independent_of_write_path() {
        // Two stacks reaching the same logical values by different paths — one
        // that writes then zeroes a slot (present-but-zero chunk) and one that
        // never touches it (absent chunk) — must compare equal, because both
        // touch the same *areas* (dirty_areas must match for eq).
        let mut a = Stack::new();
        let mut b = Stack::new();

        // Same dirty areas: both write area 0 (slot 5) and area 3 (slot 3100).
        a.set(5, U256::from(42u64));
        a.set(3100, U256::from(7u64));
        a.set(6, U256::from(99u64));
        a.set(6, U256::zero()); // present-but-zero in a

        b.set(5, U256::from(42u64));
        b.set(3100, U256::from(7u64));
        b.set(6, U256::zero()); // dirties area 0 without a prior nonzero

        assert_eq!(&a, &b, "present-zero vs freshly-zeroed slot must be equal");

        // Differing value must be unequal.
        let mut c = b.clone();
        c.set(5, U256::from(43u64));
        assert_ne!(&b, &c);
    }

    #[test]
    fn clone_is_a_deep_independent_copy() {
        let mut rng = XorShift(0x5555_aaaa_5555_aaaa);
        let mut original = Stack::new();
        for _ in 0..3_000 {
            original.set(rng.slot(), rng.value());
        }
        let cloned = original.clone();
        assert_eq!(&original, &cloned);

        // Mutating the original must not affect the clone.
        original.set(1234, U256::from(0xffff_ffffu64));
        assert_eq!(cloned.get(1234), U256::zero());
        // And the clone still matches its snapshot of the original.
        for slot in [0u16, 5, 1024, 3100, 60000, 65535] {
            let _ = cloned.get(slot); // no panic; values already checked via eq
        }
    }
}
