use std::collections::BTreeMap;

use primitive_types::{H160, U256};
use zkevm_opcode_defs::system_params::{
    STORAGE_ACCESS_COLD_READ_COST, STORAGE_ACCESS_COLD_WRITE_COST, STORAGE_ACCESS_WARM_READ_COST,
    STORAGE_ACCESS_WARM_WRITE_COST,
};
use zksync_vm2_interface::{CycleStats, Event, L2ToL1Log, Tracer};

use crate::{
    rollback::{Rollback, RollbackableLog, RollbackableMap, RollbackablePod, RollbackableSet},
    StorageInterface, StorageSlot,
};

/// Pending modifications to the global state that are executed at the end of a block.
/// In other words, side effects.
#[derive(Debug, Default)]
pub struct WorldDiff {
    // These are rolled back on revert or panic (and when the whole VM is rolled back).
    storage_changes: RollbackableMap<(H160, U256), U256>,
    paid_changes: RollbackableMap<(H160, U256), u32>,
    transient_storage_changes: RollbackableMap<(H160, U256), U256>,
    events: RollbackableLog<Event>,
    l2_to_l1_logs: RollbackableLog<L2ToL1Log>,
    pub(crate) pubdata: RollbackablePod<i32>,
    storage_refunds: RollbackableLog<u32>,
    pubdata_costs: RollbackableLog<i32>,

    // The fields below are only rolled back when the whole VM is rolled back.
    /// Values indicate whether a bytecode was successfully decommitted. When accessing decommitted hashes
    /// for the execution state, we need to track both successful and failed decommitments; OTOH, only successful ones
    /// matter when computing decommitment cost.
    pub(crate) decommitted_hashes: RollbackableMap<U256, bool>,
    read_storage_slots: RollbackableSet<(H160, U256)>,
    written_storage_slots: RollbackableSet<(H160, U256)>,

    // This is never rolled back. It is just a cache to avoid asking these from DB every time.
    storage_initial_values: BTreeMap<(H160, U256), StorageSlot>,
}

#[derive(Debug)]
pub(crate) struct ExternalSnapshot {
    internal_snapshot: Snapshot,
    pub(crate) decommitted_hashes: <RollbackableMap<U256, ()> as Rollback>::Snapshot,
    read_storage_slots: <RollbackableMap<(H160, U256), ()> as Rollback>::Snapshot,
    written_storage_slots: <RollbackableMap<(H160, U256), ()> as Rollback>::Snapshot,
    storage_refunds: <RollbackableLog<u32> as Rollback>::Snapshot,
    pubdata_costs: <RollbackableLog<i32> as Rollback>::Snapshot,
}

impl WorldDiff {
    /// Returns the storage slot's value and a refund based on its hot/cold status.
    pub(crate) fn read_storage(
        &mut self,
        world: &mut impl StorageInterface,
        tracer: &mut impl Tracer,
        contract: H160,
        key: U256,
    ) -> (U256, u32) {
        let (value, refund) = self.read_storage_inner(world, tracer, contract, key);
        self.storage_refunds.push(refund);
        (value, refund)
    }

    /// Same as [`Self::read_storage()`], but without recording the refund value (which is important
    /// because the storage is read not only from the `sload` op handler, but also from the `farcall` op handler;
    /// the latter must not record a refund as per previous VM versions).
    pub(crate) fn read_storage_without_refund(
        &mut self,
        world: &mut impl StorageInterface,
        tracer: &mut impl Tracer,
        contract: H160,
        key: U256,
    ) -> U256 {
        self.read_storage_inner(world, tracer, contract, key).0
    }

    fn read_storage_inner(
        &mut self,
        world: &mut impl StorageInterface,
        tracer: &mut impl Tracer,
        contract: H160,
        key: U256,
    ) -> (U256, u32) {
        let value = self
            .storage_changes
            .as_ref()
            .get(&(contract, key))
            .copied()
            .unwrap_or_else(|| world.read_storage_value(contract, key));

        let newly_added = self.read_storage_slots.add((contract, key));
        if newly_added {
            tracer.on_extra_prover_cycles(CycleStats::StorageRead);
        }

        let refund = if !newly_added || world.is_free_storage_slot(&contract, &key) {
            WARM_READ_REFUND
        } else {
            0
        };
        self.pubdata_costs.push(0);
        (value, refund)
    }

    /// Returns the refund based the hot/cold status of the storage slot and the change in pubdata.
    pub(crate) fn write_storage(
        &mut self,
        world: &mut impl StorageInterface,
        tracer: &mut impl Tracer,
        contract: H160,
        key: U256,
        value: U256,
    ) -> u32 {
        self.storage_changes.insert((contract, key), value);

        let initial_value = self
            .storage_initial_values
            .entry((contract, key))
            .or_insert_with(|| world.read_storage(contract, key));

        if world.is_free_storage_slot(&contract, &key) {
            if self.written_storage_slots.add((contract, key)) {
                tracer.on_extra_prover_cycles(CycleStats::StorageWrite);
            }
            self.read_storage_slots.add((contract, key));

            self.storage_refunds.push(WARM_WRITE_REFUND);
            self.pubdata_costs.push(0);
            return WARM_WRITE_REFUND;
        }

        let update_cost = world.cost_of_writing_storage(*initial_value, value);
        let prepaid = self
            .paid_changes
            .insert((contract, key), update_cost)
            .unwrap_or(0);

        let refund = if self.written_storage_slots.add((contract, key)) {
            tracer.on_extra_prover_cycles(CycleStats::StorageWrite);

            if self.read_storage_slots.add((contract, key)) {
                0
            } else {
                COLD_WRITE_AFTER_WARM_READ_REFUND
            }
        } else {
            WARM_WRITE_REFUND
        };

        #[allow(clippy::cast_possible_wrap)]
        {
            let pubdata_cost = (update_cost as i32) - (prepaid as i32);
            self.pubdata.0 += pubdata_cost;
            self.storage_refunds.push(refund);
            self.pubdata_costs.push(pubdata_cost);
        }
        refund
    }

    pub(crate) fn pubdata(&self) -> i32 {
        self.pubdata.0
    }

    /// Returns recorded refunds for all storage operations.
    pub fn storage_refunds(&self) -> &[u32] {
        self.storage_refunds.as_ref()
    }

    /// Returns recorded pubdata costs for all storage operations.
    pub fn pubdata_costs(&self) -> &[i32] {
        self.pubdata_costs.as_ref()
    }

    #[doc(hidden)] // duplicates `StateInterface::get_storage_state()`, but we use random access in some places
    pub fn get_storage_state(&self) -> &BTreeMap<(H160, U256), U256> {
        self.storage_changes.as_ref()
    }

    /// Gets changes for all touched storage slots.
    pub fn get_storage_changes(&self) -> impl Iterator<Item = ((H160, U256), StorageChange)> + '_ {
        self.storage_changes
            .as_ref()
            .iter()
            .filter_map(|(key, &value)| {
                let initial_slot = &self.storage_initial_values[key];
                if initial_slot.value == value {
                    None
                } else {
                    Some((
                        *key,
                        StorageChange {
                            before: initial_slot.value,
                            after: value,
                            is_initial: initial_slot.is_write_initial,
                        },
                    ))
                }
            })
    }

    /// Gets changes for storage slots touched after the specified `snapshot` was created.
    pub fn get_storage_changes_after(
        &self,
        snapshot: &Snapshot,
    ) -> impl Iterator<Item = ((H160, U256), StorageChange)> + '_ {
        self.storage_changes
            .changes_after(snapshot.storage_changes)
            .into_iter()
            .map(|(key, (before, after))| {
                let initial = self.storage_initial_values[&key];
                (
                    key,
                    StorageChange {
                        before: before.unwrap_or(initial.value),
                        after,
                        is_initial: initial.is_write_initial,
                    },
                )
            })
    }

    pub(crate) fn read_transient_storage(&mut self, contract: H160, key: U256) -> U256 {
        self.pubdata_costs.push(0);
        self.transient_storage_changes
            .as_ref()
            .get(&(contract, key))
            .copied()
            .unwrap_or_default()
    }

    pub(crate) fn write_transient_storage(&mut self, contract: H160, key: U256, value: U256) {
        self.pubdata_costs.push(0);
        self.transient_storage_changes
            .insert((contract, key), value);
    }

    pub(crate) fn get_transient_storage_state(&self) -> &BTreeMap<(H160, U256), U256> {
        self.transient_storage_changes.as_ref()
    }

    pub(crate) fn record_event(&mut self, event: Event) {
        self.events.push(event);
    }

    pub(crate) fn events(&self) -> &[Event] {
        self.events.as_ref()
    }

    /// Returns events emitted after the specified `snapshot` was created.
    pub fn events_after(&self, snapshot: &Snapshot) -> &[Event] {
        self.events.logs_after(snapshot.events)
    }

    pub(crate) fn record_l2_to_l1_log(&mut self, log: L2ToL1Log) {
        self.l2_to_l1_logs.push(log);
    }

    pub(crate) fn l2_to_l1_logs(&self) -> &[L2ToL1Log] {
        self.l2_to_l1_logs.as_ref()
    }

    /// Returns L2-to-L1 logs emitted after the specified `snapshot` was created.
    pub fn l2_to_l1_logs_after(&self, snapshot: &Snapshot) -> &[L2ToL1Log] {
        self.l2_to_l1_logs.logs_after(snapshot.l2_to_l1_logs)
    }

    /// Returns hashes of decommitted contract bytecodes in no particular order. Note that this includes
    /// failed (out-of-gas) decommitments.
    pub fn decommitted_hashes(&self) -> impl Iterator<Item = U256> + '_ {
        self.decommitted_hashes.as_ref().keys().copied()
    }

    /// Get a snapshot for selecting which logs & co. to output using [`Self::events_after()`] and other methods.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            storage_changes: self.storage_changes.snapshot(),
            paid_changes: self.paid_changes.snapshot(),
            events: self.events.snapshot(),
            l2_to_l1_logs: self.l2_to_l1_logs.snapshot(),
            transient_storage_changes: self.transient_storage_changes.snapshot(),
            pubdata: self.pubdata.snapshot(),
        }
    }

    #[allow(clippy::needless_pass_by_value)] // intentional: we require a snapshot to be rolled back to no more than once
    pub(crate) fn rollback(&mut self, snapshot: Snapshot) {
        self.storage_changes.rollback(snapshot.storage_changes);
        self.paid_changes.rollback(snapshot.paid_changes);
        self.events.rollback(snapshot.events);
        self.l2_to_l1_logs.rollback(snapshot.l2_to_l1_logs);
        self.transient_storage_changes
            .rollback(snapshot.transient_storage_changes);
        self.pubdata.rollback(snapshot.pubdata);
    }

    /// This function must only be called during the initial frame
    /// because otherwise internal rollbacks can roll back past the external snapshot.
    pub(crate) fn external_snapshot(&self) -> ExternalSnapshot {
        // Rolling back to this snapshot will clear transient storage even though it is not empty
        // after a transaction. This is ok because the next instruction in the bootloader
        // (IncrementTxNumber) clears the transient storage anyway.
        // This is necessary because clear_transient_storage cannot be undone.
        ExternalSnapshot {
            internal_snapshot: Snapshot {
                transient_storage_changes: 0,
                ..self.snapshot()
            },
            decommitted_hashes: self.decommitted_hashes.snapshot(),
            read_storage_slots: self.read_storage_slots.snapshot(),
            written_storage_slots: self.written_storage_slots.snapshot(),
            storage_refunds: self.storage_refunds.snapshot(),
            pubdata_costs: self.pubdata_costs.snapshot(),
        }
    }

    pub(crate) fn external_rollback(&mut self, snapshot: ExternalSnapshot) {
        self.rollback(snapshot.internal_snapshot);
        self.storage_refunds.rollback(snapshot.storage_refunds);
        self.pubdata_costs.rollback(snapshot.pubdata_costs);
        self.decommitted_hashes
            .rollback(snapshot.decommitted_hashes);
        self.read_storage_slots
            .rollback(snapshot.read_storage_slots);
        self.written_storage_slots
            .rollback(snapshot.written_storage_slots);
    }

    pub(crate) fn delete_history(&mut self) {
        self.storage_changes.delete_history();
        self.paid_changes.delete_history();
        self.transient_storage_changes.delete_history();
        self.events.delete_history();
        self.l2_to_l1_logs.delete_history();
        self.pubdata.delete_history();
        self.storage_refunds.delete_history();
        self.pubdata_costs.delete_history();
        self.decommitted_hashes.delete_history();
        self.read_storage_slots.delete_history();
        self.written_storage_slots.delete_history();
    }

    pub(crate) fn clear_transient_storage(&mut self) {
        self.transient_storage_changes = RollbackableMap::default();
    }
}

/// Opaque snapshot of a [`WorldDiff`] output by its [eponymous method](WorldDiff::snapshot()).
/// Can be provided to [`WorldDiff::events_after()`] etc. to get data after the snapshot was created.
#[derive(Clone, PartialEq, Debug)]
pub struct Snapshot {
    storage_changes: <RollbackableMap<(H160, U256), U256> as Rollback>::Snapshot,
    paid_changes: <RollbackableMap<(H160, U256), u32> as Rollback>::Snapshot,
    events: <RollbackableLog<Event> as Rollback>::Snapshot,
    l2_to_l1_logs: <RollbackableLog<L2ToL1Log> as Rollback>::Snapshot,
    transient_storage_changes: <RollbackableMap<(H160, U256), U256> as Rollback>::Snapshot,
    pubdata: <RollbackablePod<i32> as Rollback>::Snapshot,
}

/// Change in a single storage slot.
#[derive(Debug, PartialEq)]
pub struct StorageChange {
    /// Value before the slot was written to.
    pub before: U256,
    /// Value written to the slot.
    pub after: U256,
    /// `true` if the slot is not set in the [`World`](crate::World).
    /// A write may be initial even if it isn't the first write to a slot!
    pub is_initial: bool,
}

const WARM_READ_REFUND: u32 = STORAGE_ACCESS_COLD_READ_COST - STORAGE_ACCESS_WARM_READ_COST;
const WARM_WRITE_REFUND: u32 = STORAGE_ACCESS_COLD_WRITE_COST - STORAGE_ACCESS_WARM_WRITE_COST;
const COLD_WRITE_AFTER_WARM_READ_REFUND: u32 = STORAGE_ACCESS_COLD_READ_COST;

#[cfg(test)]
mod tests {
    use proptest::{bits, collection::btree_map, prelude::*};

    use super::*;
    use crate::StorageSlot;

    fn test_storage_changes(
        initial_values: &BTreeMap<(H160, U256), StorageSlot>,
        first_changes: BTreeMap<(H160, U256), U256>,
        second_changes: BTreeMap<(H160, U256), U256>,
    ) {
        let mut world_diff = WorldDiff {
            storage_initial_values: initial_values.clone(),
            ..WorldDiff::default()
        };

        let checkpoint1 = world_diff.snapshot();
        for (key, value) in &first_changes {
            world_diff.write_storage(&mut NoWorld, &mut (), key.0, key.1, *value);
        }
        let actual_changes = world_diff
            .get_storage_changes_after(&checkpoint1)
            .collect::<BTreeMap<_, _>>();
        let expected_changes = first_changes
            .iter()
            .map(|(key, value)| {
                let before = initial_values
                    .get(key)
                    .map_or_else(U256::zero, |slot| slot.value);
                let is_initial = initial_values
                    .get(key)
                    .map_or(true, |slot| slot.is_write_initial);
                (
                    *key,
                    StorageChange {
                        before,
                        after: *value,
                        is_initial,
                    },
                )
            })
            .collect();
        assert_eq!(actual_changes, expected_changes);

        let checkpoint2 = world_diff.snapshot();
        for (key, value) in &second_changes {
            world_diff.write_storage(&mut NoWorld, &mut (), key.0, key.1, *value);
        }
        let actual_changes = world_diff
            .get_storage_changes_after(&checkpoint2)
            .collect::<BTreeMap<_, _>>();
        let expected_changes = second_changes
            .iter()
            .map(|(key, value)| {
                let before = first_changes
                    .get(key)
                    .or(initial_values.get(key).map(|slot| &slot.value))
                    .copied()
                    .unwrap_or_default();
                let is_initial = initial_values
                    .get(key)
                    .map_or(true, |slot| slot.is_write_initial);
                (
                    *key,
                    StorageChange {
                        before,
                        after: *value,
                        is_initial,
                    },
                )
            })
            .collect();
        assert_eq!(actual_changes, expected_changes);

        let mut combined = first_changes
            .into_iter()
            .filter_map(|(key, value)| {
                let initial = initial_values
                    .get(&key)
                    .copied()
                    .unwrap_or(StorageSlot::EMPTY);
                (initial.value != value).then_some((
                    key,
                    StorageChange {
                        before: initial.value,
                        after: value,
                        is_initial: initial.is_write_initial,
                    },
                ))
            })
            .collect::<BTreeMap<_, _>>();
        for (key, value) in second_changes {
            let initial = initial_values
                .get(&key)
                .copied()
                .unwrap_or(StorageSlot::EMPTY);
            if initial.value == value {
                combined.remove(&key);
            } else {
                combined.insert(
                    key,
                    StorageChange {
                        before: initial.value,
                        after: value,
                        is_initial: initial.is_write_initial,
                    },
                );
            }
        }

        assert_eq!(combined, world_diff.get_storage_changes().collect());
    }

    proptest! {
        #[test]
        fn storage_changes_work_as_expected(
            initial_values in arbitrary_initial_storage(),
            first_changes in arbitrary_storage_changes(),
            second_changes in arbitrary_storage_changes(),
        ) {
            test_storage_changes(&initial_values, first_changes, second_changes);
        }

        #[test]
        fn storage_changes_work_with_constrained_changes(
            initial_values in constrained_initial_storage(),
            first_changes in constrained_storage_changes(),
            second_changes in constrained_storage_changes(),
        ) {
            test_storage_changes(&initial_values, first_changes, second_changes);
        }
    }

    /// Max items in generated initial storage / changes.
    const MAX_ITEMS: usize = 5;
    /// Bit mask for bytes in constrained `U256` / `H160` values.
    const BIT_MASK: u8 = 0b_1111;

    fn arbitrary_initial_storage() -> impl Strategy<Value = BTreeMap<(H160, U256), StorageSlot>> {
        btree_map(
            any::<([u8; 20], [u8; 32])>()
                .prop_map(|(contract, key)| (H160::from(contract), U256::from(key))),
            any::<([u8; 32], bool)>().prop_map(|(value, is_write_initial)| StorageSlot {
                value: U256::from(value),
                is_write_initial,
            }),
            0..=MAX_ITEMS,
        )
    }

    fn constrained_initial_storage() -> impl Strategy<Value = BTreeMap<(H160, U256), StorageSlot>> {
        btree_map(
            (bits::u8::masked(BIT_MASK), bits::u8::masked(BIT_MASK))
                .prop_map(|(contract, key)| (H160::repeat_byte(contract), U256::from(key))),
            (bits::u8::masked(BIT_MASK), any::<bool>()).prop_map(|(value, is_write_initial)| {
                StorageSlot {
                    value: U256::from(value),
                    is_write_initial,
                }
            }),
            0..=MAX_ITEMS,
        )
    }

    fn arbitrary_storage_changes() -> impl Strategy<Value = BTreeMap<(H160, U256), U256>> {
        btree_map(
            any::<([u8; 20], [u8; 32])>()
                .prop_map(|(contract, key)| (H160::from(contract), U256::from(key))),
            any::<[u8; 32]>().prop_map(U256::from),
            0..=MAX_ITEMS,
        )
    }

    fn constrained_storage_changes() -> impl Strategy<Value = BTreeMap<(H160, U256), U256>> {
        btree_map(
            (bits::u8::masked(BIT_MASK), bits::u8::masked(BIT_MASK))
                .prop_map(|(contract, key)| (H160::repeat_byte(contract), U256::from(key))),
            bits::u8::masked(BIT_MASK).prop_map(U256::from),
            0..=MAX_ITEMS,
        )
    }

    struct NoWorld;

    impl StorageInterface for NoWorld {
        fn read_storage(&mut self, _: H160, _: U256) -> StorageSlot {
            StorageSlot::EMPTY
        }

        fn cost_of_writing_storage(&mut self, _: StorageSlot, _: U256) -> u32 {
            0
        }

        fn is_free_storage_slot(&self, _: &H160, _: &U256) -> bool {
            false
        }
    }
}
