use std::collections::BTreeMap;

use crate::{
    rollback::{Rollback, RollbackableLog, RollbackableMap, RollbackablePod, RollbackableSet},
    World,
};
use u256::{H160, U256};
use zkevm_opcode_defs::system_params::{
    STORAGE_ACCESS_COLD_READ_COST, STORAGE_ACCESS_COLD_WRITE_COST, STORAGE_ACCESS_WARM_READ_COST,
    STORAGE_ACCESS_WARM_WRITE_COST,
};

/// Pending modifications to the global state that are executed at the end of a block.
/// In other words, side effects.
#[derive(Default)]
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
    pub(crate) decommitted_hashes: RollbackableSet<U256>,
    read_storage_slots: RollbackableSet<(H160, U256)>,
    written_storage_slots: RollbackableSet<(H160, U256)>,

    // This is never rolled back. It is just a cache to avoid asking these from DB every time.
    storage_initial_values: BTreeMap<(H160, U256), Option<U256>>,
}

pub struct ExternalSnapshot {
    internal_snapshot: Snapshot,
    pub(crate) decommitted_hashes: <RollbackableMap<U256, ()> as Rollback>::Snapshot,
    read_storage_slots: <RollbackableMap<(H160, U256), ()> as Rollback>::Snapshot,
    written_storage_slots: <RollbackableMap<(H160, U256), ()> as Rollback>::Snapshot,
    storage_refunds: <RollbackableLog<u32> as Rollback>::Snapshot,
    pubdata_costs: <RollbackableLog<i32> as Rollback>::Snapshot,
}

/// There is no address field because nobody is interested in events that don't come
/// from the event writer, so we simply do not record events coming frome anywhere else.
#[derive(Clone, PartialEq, Debug)]
pub struct Event {
    pub key: U256,
    pub value: U256,
    pub is_first: bool,
    pub shard_id: u8,
    pub tx_number: u16,
}

#[derive(Debug)]
pub struct L2ToL1Log {
    pub key: U256,
    pub value: U256,
    pub is_service: bool,
    pub address: H160,
    pub shard_id: u8,
    pub tx_number: u16,
}

impl WorldDiff {
    /// Returns the storage slot's value and a refund based on its hot/cold status.
    pub(crate) fn read_storage(
        &mut self,
        world: &mut dyn World,
        contract: H160,
        key: U256,
    ) -> (U256, u32) {
        let (value, refund) = self.read_storage_inner(world, contract, key);
        self.storage_refunds.push(refund);
        (value, refund)
    }

    /// Same as [`Self::read_storage()`], but without recording the refund value (which is important
    /// because the storage is read not only from the `sload` op handler, but also from the `farcall` op handler;
    /// the latter must not record a refund as per previous VM versions).
    pub(crate) fn read_storage_without_refund(
        &mut self,
        world: &mut dyn World,
        contract: H160,
        key: U256,
    ) -> U256 {
        self.read_storage_inner(world, contract, key).0
    }

    fn read_storage_inner(
        &mut self,
        world: &mut dyn World,
        contract: H160,
        key: U256,
    ) -> (U256, u32) {
        let value = self
            .storage_changes
            .as_ref()
            .get(&(contract, key))
            .copied()
            .unwrap_or_else(|| world.read_storage(contract, key).unwrap_or_default());

        let refund = if world.is_free_storage_slot(&contract, &key)
            || self.read_storage_slots.contains(&(contract, key))
        {
            WARM_READ_REFUND
        } else {
            self.read_storage_slots.add((contract, key));
            0
        };
        self.pubdata_costs.push(0);
        (value, refund)
    }

    /// Returns the refund based the hot/cold status of the storage slot and the change in pubdata.
    pub(crate) fn write_storage(
        &mut self,
        world: &mut dyn World,
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
            self.storage_refunds.push(WARM_WRITE_REFUND);
            self.pubdata_costs.push(0);
            return WARM_WRITE_REFUND;
        }

        let update_cost = world.cost_of_writing_storage(*initial_value, value);
        let prepaid = self
            .paid_changes
            .insert((contract, key), update_cost)
            .unwrap_or(0);

        let refund = if self
            .written_storage_slots
            .as_ref()
            .contains_key(&(contract, key))
        {
            WARM_WRITE_REFUND
        } else {
            self.written_storage_slots.add((contract, key));

            if self.read_storage_slots.contains(&(contract, key)) {
                COLD_WRITE_AFTER_WARM_READ_REFUND
            } else {
                self.read_storage_slots.add((contract, key));
                0
            }
        };

        let pubdata_cost = (update_cost as i32) - (prepaid as i32);
        self.pubdata.0 += pubdata_cost;
        self.storage_refunds.push(refund);
        self.pubdata_costs.push(pubdata_cost);
        refund
    }

    pub fn pubdata(&self) -> i32 {
        self.pubdata.0
    }

    pub fn storage_refunds(&self) -> &[u32] {
        self.storage_refunds.as_ref()
    }

    pub fn pubdata_costs(&self) -> &[i32] {
        self.pubdata_costs.as_ref()
    }

    pub fn get_storage_state(&self) -> &BTreeMap<(H160, U256), U256> {
        self.storage_changes.as_ref()
    }

    pub fn get_storage_changes(
        &self,
    ) -> impl Iterator<Item = ((H160, U256), (Option<U256>, U256))> + '_ {
        self.storage_changes
            .as_ref()
            .iter()
            .filter_map(|(key, &value)| {
                if self.storage_initial_values[key].unwrap_or_default() == value {
                    None
                } else {
                    Some((*key, (self.storage_initial_values[key], value)))
                }
            })
    }

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
                        before: before.or(initial),
                        after,
                        is_initial: initial.is_none(),
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

    pub(crate) fn record_event(&mut self, event: Event) {
        self.events.push(event);
    }

    pub fn events(&self) -> &[Event] {
        self.events.as_ref()
    }

    pub fn events_after(&self, snapshot: &Snapshot) -> &[Event] {
        self.events.logs_after(snapshot.events)
    }

    pub(crate) fn record_l2_to_l1_log(&mut self, log: L2ToL1Log) {
        self.l2_to_l1_logs.push(log);
    }

    pub fn l2_to_l1_logs(&self) -> &[L2ToL1Log] {
        self.l2_to_l1_logs.as_ref()
    }

    pub fn l2_to_l1_logs_after(&self, snapshot: &Snapshot) -> &[L2ToL1Log] {
        self.l2_to_l1_logs.logs_after(snapshot.l2_to_l1_logs)
    }

    /// Returns hashes of decommitted contract bytecodes in no particular order.
    pub fn decommitted_hashes(&self) -> impl Iterator<Item = U256> + '_ {
        self.decommitted_hashes.as_ref().keys().copied()
    }

    /// Get a snapshot for selecting which logs [Self::events_after] & Co output.
    /// The snapshot can't be used for rolling back the VM because the method for
    /// that is private. Use [crate::VirtualMachine::snapshot] for that instead.
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

    /// This must only be called when it is known that the VM cannot be rolled back,
    /// so there must not be any external snapshots and the callstack
    /// should ideally be empty, though in practice it sometimes contains
    /// a near call inside the bootloader.
    pub fn delete_history(&mut self) {
        self.storage_changes.delete_history();
        self.events.delete_history();
        self.decommitted_hashes.delete_history();
        self.read_storage_slots.delete_history();
        self.written_storage_slots.delete_history();
    }

    pub(crate) fn clear_transient_storage(&mut self) {
        self.transient_storage_changes = Default::default();
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Snapshot {
    storage_changes: <RollbackableMap<(H160, U256), U256> as Rollback>::Snapshot,
    paid_changes: <RollbackableMap<(H160, U256), u32> as Rollback>::Snapshot,
    events: <RollbackableLog<Event> as Rollback>::Snapshot,
    l2_to_l1_logs: <RollbackableLog<L2ToL1Log> as Rollback>::Snapshot,
    transient_storage_changes: <RollbackableMap<(H160, U256), U256> as Rollback>::Snapshot,
    pubdata: <RollbackablePod<i32> as Rollback>::Snapshot,
}

#[derive(Debug, PartialEq)]
pub struct StorageChange {
    pub before: Option<U256>,
    pub after: U256,
    /// `true` if the slot is not set in the World.
    /// A write may be initial even if it isn't the first write to a slot!
    pub is_initial: bool,
}

const WARM_READ_REFUND: u32 = STORAGE_ACCESS_COLD_READ_COST - STORAGE_ACCESS_WARM_READ_COST;
const WARM_WRITE_REFUND: u32 = STORAGE_ACCESS_COLD_WRITE_COST - STORAGE_ACCESS_WARM_WRITE_COST;
const COLD_WRITE_AFTER_WARM_READ_REFUND: u32 = STORAGE_ACCESS_COLD_READ_COST;

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn test_storage_changes(
            initial_values in arbitrary_storage_changes(),
            first_changes in arbitrary_storage_changes(),
            second_changes in arbitrary_storage_changes(),
        ) {
            let mut world_diff = WorldDiff::default();
            world_diff.storage_initial_values = initial_values
                .iter()
                .map(|(key, value)| (*key, Some(*value)))
                .collect();

            let checkpoint1 = world_diff.snapshot();
            for (key, value) in &first_changes {
                world_diff.write_storage(&mut NoWorld, key.0, key.1, *value);
            }
            assert_eq!(
                world_diff
                    .get_storage_changes_after(&checkpoint1)
                    .collect::<BTreeMap<_, _>>(),
                first_changes
                    .iter()
                    .map(|(key, value)| (
                        *key,
                        StorageChange {
                            before: initial_values.get(key).copied(),
                            after: *value,
                            is_initial: initial_values.get(key).is_none(),
                        }
                    ))
                    .collect()
            );

            let checkpoint2 = world_diff.snapshot();
            for (key, value) in &second_changes {
                world_diff.write_storage(&mut NoWorld, key.0, key.1, *value);
            }
            assert_eq!(
                world_diff
                    .get_storage_changes_after(&checkpoint2)
                    .collect::<BTreeMap<_, _>>(),
                second_changes
                    .iter()
                    .map(|(key, value)| (
                        *key,
                        StorageChange {
                            before: first_changes.get(key).or(initial_values.get(key)).copied(),
                            after: *value,
                            is_initial: initial_values.get(key).is_none(),
                        }
                    ))
                    .collect()
            );

            let mut combined = first_changes
                .into_iter()
                .filter_map(|(key, value)| {
                    let initial = initial_values.get(&key).copied();
                    (initial.unwrap_or_default() != value).then_some((key, (initial, value)))
                })
                .collect::<BTreeMap<_, _>>();
            for (key, value) in second_changes {
                let initial = initial_values.get(&key).copied();
                if initial.unwrap_or_default() != value {
                    combined.insert(key, (initial, value));
                } else {
                    combined.remove(&key);
                }
            }

            assert_eq!(combined, world_diff.get_storage_changes().collect());
        }
    }

    fn arbitrary_storage_changes() -> impl Strategy<Value = BTreeMap<(H160, U256), U256>> {
        any::<Vec<(([u8; 20], [u8; 32]), [u8; 32])>>().prop_map(|vec| {
            vec.into_iter()
                .map(|((contract, key), value)| {
                    ((H160::from(contract), U256::from(key)), U256::from(value))
                })
                .collect()
        })
    }

    struct NoWorld;
    impl World for NoWorld {
        fn decommit(&mut self, _: U256) -> crate::Program {
            unimplemented!()
        }

        fn read_storage(&mut self, _: H160, _: U256) -> Option<U256> {
            None
        }

        fn cost_of_writing_storage(&mut self, _: Option<U256>, _: U256) -> u32 {
            0
        }

        fn is_free_storage_slot(&self, _: &H160, _: &U256) -> bool {
            false
        }

        fn decommit_code(&mut self, _: U256) -> Vec<u8> {
            unimplemented!()
        }
    }
}
