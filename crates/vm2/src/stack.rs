use std::{
    alloc::{alloc_zeroed, handle_alloc_error, Layout},
    fmt,
};

use primitive_types::U256;

use crate::{bitset::Bitset, fat_pointer::FatPointer, hash_for_debugging};

const NUMBER_OF_DIRTY_AREAS: usize = 64;
const DIRTY_AREA_SIZE: usize = (1 << 16) / NUMBER_OF_DIRTY_AREAS;

// Slots live in lazily-allocated sub-chunks of `SUBCHUNK_SLOTS` slots: a frame
// allocates one sub-chunk per *distinct* sub-chunk it writes, so allocation is
// proportional to the sub-chunks it touches rather than to the full 2 MiB of
// slots: a frame writing few, clustered slots costs a few KB, while one writing
// a slot in every sub-chunk materializes all `NUM_SUBCHUNKS` of them — the same
// 2 MiB the dense layout paid unconditionally, which is the cap. Once
// materialized, a sub-chunk is kept for the stack's lifetime and cleared in
// place on reuse — freeing it would let guest programs trigger unpriced
// allocator work on every far call. Dirty tracking stays at the coarse
// `dirty_areas` level, so snapshot/rollback/equality semantics match the dense
// stack exactly.
const SUBCHUNK_SLOTS: usize = 16;
const NUM_SUBCHUNKS: usize = (1 << 16) / SUBCHUNK_SLOTS;
const SUBCHUNKS_PER_AREA: usize = DIRTY_AREA_SIZE / SUBCHUNK_SLOTS;

/// A contiguous block of `SUBCHUNK_SLOTS` stack slots, allocated on demand.
type SlotChunk = Box<[U256; SUBCHUNK_SLOTS]>;

/// Allocates a zeroed chunk directly on the heap (all-zero bits are a valid
/// `U256` array; `Box::new` would materialize it on the caller's stack first).
#[allow(clippy::cast_ptr_alignment)] // aligned per the array layout
fn zeroed_chunk() -> SlotChunk {
    let layout = Layout::new::<[U256; SUBCHUNK_SLOTS]>();
    let ptr = unsafe { alloc_zeroed(layout) };
    if ptr.is_null() {
        handle_alloc_error(layout);
    }
    unsafe { Box::from_raw(ptr.cast()) }
}

/// VM stack: 2^16 slots stored as [`NUM_SUBCHUNKS`] lazily-allocated sub-chunks.
///
/// An absent sub-chunk reads as all-zero and is indistinguishable from a
/// materialized all-zero one, so the layout is invisible to VM behavior. A
/// stack keeps its materialized sub-chunks for its whole lifetime — its memory
/// is its high-water mark — and `zero()` clears them in place for reuse.
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
        let layout = Layout::new::<Self>();
        let ptr = unsafe { alloc_zeroed(layout) };
        if ptr.is_null() {
            handle_alloc_error(layout);
        }
        unsafe { Box::from_raw(ptr.cast()) }
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
        // Every write dirties its area — including zero writes — matching the
        // dense implementation, so `dirty_areas` evolves identically.
        self.dirty_areas |= 1 << area;
        let subchunk = slot as usize / SUBCHUNK_SLOTS;
        let chunk = self.slots[subchunk].get_or_insert_with(zeroed_chunk);
        chunk[slot as usize % SUBCHUNK_SLOTS] = value;
    }

    fn zero(&mut self) {
        // Clear materialized sub-chunks in place instead of dropping them:
        // freeing would round-trip the allocator up to `NUM_SUBCHUNKS` times
        // per pooled-stack reuse — unpriced host work a guest program can
        // trigger on every far call. Clearing only dirty areas suffices: a
        // sub-chunk becomes nonzero only via `set` (which dirties its area) or
        // `rollback` (which restores `dirty_areas` alongside the data), so
        // chunks in non-dirty areas are already all-zero.
        //
        // The work is proportional to the materialized chunks in the dirty
        // areas — the stack's high-water mark, not the writes of the frame being
        // recycled — and is bounded above by what the dense layout memset for
        // the same dirty mask, which cleared every slot of a dirty area whether
        // it had been written or not.
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

        debug_assert!(
            self.slots
                .iter()
                .flatten()
                .all(|chunk| chunk.iter().all(U256::is_zero)),
            "nonzero sub-chunk survived zero(): `nonzero => dirty` was violated"
        );
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
        // Restore the sub-chunks of the snapshot's dirty areas (all of which
        // lie within the saved prefix), reusing chunks that are already
        // materialized and skipping ones that would be created just to hold
        // zeros — absent already reads as zero. Non-dirty areas are all-zero
        // after the `zero()` above.
        //
        // Retention is therefore workload-dependent: rolling back to a snapshot
        // whose `dirty_areas` is narrower than the current one keeps the chunks
        // materialized outside it. They hold zeros, so `nonzero => dirty` still
        // holds and they stay invisible to `get`/`PartialEq`/`snapshot`.
        for i in 0..NUMBER_OF_DIRTY_AREAS {
            if dirty_areas & (1 << i) != 0 {
                for sc in (i * SUBCHUNKS_PER_AREA)..((i + 1) * SUBCHUNKS_PER_AREA) {
                    let src = &slots[sc * SUBCHUNK_SLOTS..(sc + 1) * SUBCHUNK_SLOTS];
                    if self.slots[sc].is_none() && src.iter().all(U256::is_zero) {
                        continue;
                    }
                    self.slots[sc]
                        .get_or_insert_with(zeroed_chunk)
                        .copy_from_slice(src);
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

/// Pool of stacks reused across frames, LIFO.
///
/// [`Stack::zero`] runs when the pool hands a stack *out* ([`Self::get`]), never
/// when a frame hands one back ([`Self::recycle`]). Two consequences are easy to
/// get backwards: the pool does not shrink as a deep call chain unwinds — those
/// stacks go back into it materialized and stay that way — and clearing on reuse
/// only ever reaches entries a later workload actually re-pops, so a workload
/// that stays shallow leaves deeper entries as they were. A pooled stack's
/// footprint is therefore its high-water mark for the lifetime of the pool.
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
        /// 1-in-8 zero values: zero writes must dirty/allocate like nonzero ones.
        fn value_or_zero(&mut self) -> U256 {
            if self.next().is_multiple_of(8) {
                U256::zero()
            } else {
                self.value()
            }
        }
    }

    const DENSE: usize = 1 << 16;

    #[test]
    fn differential_random_set_get_matches_dense_oracle() {
        let mut rng = XorShift(0x1234_5678_9abc_def1);
        let mut stack = Stack::new();
        let mut oracle = vec![U256::zero(); DENSE];

        for _ in 0..20_000 {
            let (slot, value) = (rng.slot(), rng.value_or_zero());
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
        // `eq` covers slot values, dirty bits, and pointer flags.
        assert_eq!(&stack, &Stack::new(), "zero() must equal a fresh stack");
    }

    #[test]
    fn zero_retains_materialized_chunks() {
        // `zero()` must clear sub-chunks in place, not free them: dropping
        // them here reintroduces per-far-call allocator churn on pooled reuse.
        let slots = [0u16, 17, 1000, 4096, 32_768, 65_535]; // scattered, incl. area boundaries
        let mut stack = Stack::new();
        for (i, slot) in slots.into_iter().enumerate() {
            stack.set(slot, U256::from(u64::try_from(i).unwrap() + 1));
        }
        let materialized: Vec<usize> = (0..NUM_SUBCHUNKS)
            .filter(|&sc| stack.slots[sc].is_some())
            .collect();
        let mut expected: Vec<usize> = slots
            .iter()
            .map(|&s| usize::from(s) / SUBCHUNK_SLOTS)
            .collect();
        expected.sort_unstable(); // `materialized` is in ascending index order
        assert_eq!(materialized, expected, "each write hits its own sub-chunk");

        stack.zero();
        for &sc in &materialized {
            let chunk = stack.slots[sc]
                .as_ref()
                .unwrap_or_else(|| panic!("zero() must keep sub-chunk {sc}, not free it"));
            assert!(
                chunk.iter().all(U256::is_zero),
                "sub-chunk {sc} not cleared"
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
            let (slot, value) = (rng.slot(), rng.value_or_zero());
            reused.set(slot, value);
            fresh.set(slot, value);
        }
        assert_eq!(&reused, &fresh, "reuse must not be observable");
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
        // Start from a recycled stack — the state `StackPool::get` hands out.
        let mut stack = Stack::new();
        for _ in 0..4_000 {
            stack.set(rng.slot(), rng.value());
        }
        stack.zero();

        let mut oracle = vec![U256::zero(); DENSE];
        for _ in 0..8_000 {
            let (slot, value) = (rng.slot(), rng.value_or_zero());
            stack.set(slot, value);
            oracle[usize::from(slot)] = value;
        }
        let snap = stack.snapshot();

        // Diverge: overwrite, zero-write, and touch new areas.
        for _ in 0..8_000 {
            stack.set(rng.slot(), rng.value());
        }
        stack.set(0xffff, rng.value());
        stack.set(0, U256::zero());

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
    fn rollback_narrowing_dirty_areas_restores_flags_and_reuses_chunks() {
        // The one transition that can strand state: a rollback that NARROWS
        // `dirty_areas`, leaving a materialized (all-zero) chunk in a
        // non-dirty area. Also pins rollback's pointer-flag restoration,
        // in-place chunk reuse, and the skip of all-zero materializations.
        let mut stack = Stack::new();
        stack.set(5, U256::from(42u64)); // area 0, sub-chunk 0
        stack.set_pointer_flag(5);
        let chunk_ptr = stack.slots[0].as_ref().unwrap().as_ptr();
        let snap = stack.snapshot();

        // Diverge into area 63, including pointer-flag churn.
        stack.set(u16::MAX, U256::from(7u64));
        stack.set_pointer_flag(u16::MAX);
        stack.clear_pointer_flag(5);
        stack.rollback(snap);

        assert_eq!(stack.dirty_areas, 1, "dirty_areas must narrow to {{0}}");
        assert_eq!(stack.get(5), U256::from(42u64));
        assert_eq!(
            stack.get(u16::MAX),
            U256::zero(),
            "diverged write must be gone"
        );
        assert!(stack.get_pointer_flag(5), "cleared flag must be restored");
        assert!(
            !stack.get_pointer_flag(u16::MAX),
            "diverged flag must be gone"
        );
        // Rollback reuses the materialized chunk in place (no realloc churn)…
        assert_eq!(stack.slots[0].as_ref().unwrap().as_ptr(), chunk_ptr);
        // …and materializes nothing just to hold zeros: only sub-chunk 0 and
        // the diverged-then-zeroed one at the top may exist.
        assert_eq!(stack.slots.iter().flatten().count(), 2);

        // The retained chunk for `u16::MAX` now sits in a NON-dirty area; the
        // next recycle must still hand out a fresh-equal stack.
        stack.zero();
        assert_eq!(&stack, &Stack::new());
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
    }
}
